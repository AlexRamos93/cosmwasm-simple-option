#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{State, STATE};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:simple-option";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    if msg.expires <= _env.block.height {
        return Err(ContractError::Expired {});
    }
    let state = State {
        creator: info.sender.clone(),
        owner: info.sender.clone(),
        collateral: info.funds,
        counter_offer: msg.counter_offer,
        expires: msg.expires,
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Transfer { recipient } => try_transfer(deps, info, recipient),
        ExecuteMsg::Execute {} => try_execute(deps, _env, info),
        ExecuteMsg::Burn {} => try_burn(deps, _env, info),
    }
}

pub fn try_transfer(
    deps: DepsMut,
    info: MessageInfo,
    recipient: Addr,
) -> Result<Response, ContractError> {
    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        if info.sender != state.owner {
            return Err(ContractError::Unauthorized {});
        }
        state.owner = recipient.clone();
        Ok(state)
    })?;

    Ok(Response::new()
        .add_attribute("method", "try_transfer")
        .add_attribute("new owner", recipient.clone()))
}

pub fn try_execute(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }
    if env.block.height >= state.expires {
        return Err(ContractError::Expired {});
    }
    if info.funds != state.counter_offer {
        return Err(ContractError::DiffCounterOffer {
            counter_offer: format!("{:?}", state.counter_offer),
        });
    }

    let res = Response::new()
        .add_message(BankMsg::Send {
            to_address: state.creator.to_string(),
            amount: state.counter_offer.clone(),
        })
        .add_message(BankMsg::Send {
            to_address: state.owner.to_string(),
            amount: state.collateral,
        });

    STATE.remove(deps.storage);

    Ok(res.add_attribute("method", "try_execute"))
}

pub fn try_burn(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    if env.block.height < state.expires {
        return Err(ContractError::CustomError {
            val: "Option not yet expired".to_string(),
        });
    }
    if !info.funds.is_empty() {
        return Err(ContractError::CustomError {
            val: "dont send funds with burn".to_string(),
        });
    }

    let res = Response::new().add_message(BankMsg::Send {
        to_address: state.creator.to_string(),
        amount: state.collateral,
    });
    Ok(res.add_attribute("method", "try_burn"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies_with_balance, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary, Attribute, SubMsg};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));
        let msg = InstantiateMsg {
            counter_offer: coins(40, "ETH"),
            expires: 100_000,
        };
        let info = mock_info("creator", &coins(1, "BTC"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        let res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();

        let value: ConfigResponse = from_binary(&res).unwrap();

        assert_eq!(100_000, value.expires);
        assert_eq!("creator", value.owner);
        assert_eq!("creator", value.creator);
        assert_eq!(coins(1, "BTC"), value.collateral);
        assert_eq!(coins(40, "ETH"), value.counter_offer);
    }

    #[test]
    fn transfer() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
            counter_offer: coins(40, "ETH"),
            expires: 100_000,
        };
        let info = mock_info("creator", &coins(1, "BTC"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        let _res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();

        // random cant transfer
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::Transfer {
            recipient: Addr::unchecked("anyone"),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        match err {
            ContractError::Unauthorized {} => {}
            _ => panic!("Must return unauthorized error"),
        }

        // owner can transfer
        let _info = mock_info("creator", &[]);
        let _msg = ExecuteMsg::Transfer {
            recipient: Addr::unchecked("someone"),
        };
        let success = execute(deps.as_mut(), mock_env(), _info, _msg).unwrap();
        assert_eq!(success.attributes.len(), 2);
        assert_eq!(
            success.attributes[1],
            Attribute::new("new owner", "someone")
        );

        let res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();
        let value: ConfigResponse = from_binary(&res).unwrap();

        assert_eq!("someone", value.owner);
        assert_eq!("creator", value.creator);
    }

    #[test]
    fn execute_test() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let counter_offer = coins(40, "ETH");
        let msg = InstantiateMsg {
            counter_offer: counter_offer.clone(),
            expires: 100_000,
        };
        let collateral = coins(1, "BTC");
        let info = mock_info("creator", &collateral);
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(0, res.messages.len());
        let _res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();

        // random cant execute
        let info = mock_info("anyone", &counter_offer);
        let msg = ExecuteMsg::Transfer {
            recipient: Addr::unchecked("anyone"),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        match err {
            ContractError::Unauthorized {} => {}
            _ => panic!("Must return unauthorized error"),
        }

        // expired cannot execute
        let _info = mock_info("creator", &counter_offer);
        let _msg = ExecuteMsg::Execute {};

        let mut env = mock_env();
        env.block.height = 200_000;
        let _err = execute(deps.as_mut(), env, _info, _msg).unwrap_err();
        match _err {
            ContractError::Expired {} => {}
            _ => panic!("Must return unauthorized error"),
        }

        // wrong counter_offer cannot execute
        let _info = mock_info("creator", &coins(39, "ADA"));
        let _msg = ExecuteMsg::Execute {};
        let _err = execute(deps.as_mut(), mock_env(), _info, _msg).unwrap_err();
        match _err {
            ContractError::DiffCounterOffer { counter_offer } => assert_eq!(
                format!("Must send exact counter_offer: {}", counter_offer),
                "Must send exact counter_offer: [Coin { denom: \"ETH\", amount: Uint128(40) }]"
            ),
            e => panic!("unexpected error: {}", e),
        }

        // proper execution
        let _info = mock_info("creator", &counter_offer);
        let _msg = ExecuteMsg::Execute {};
        let success = execute(deps.as_mut(), mock_env(), _info, _msg).unwrap();
        assert_eq!(success.messages.len(), 2);
        assert_eq!(
            success.messages[0],
            SubMsg::new(BankMsg::Send {
                to_address: "creator".into(),
                amount: counter_offer.clone(),
            })
        );
        assert_eq!(
            success.messages[1],
            SubMsg::new(BankMsg::Send {
                to_address: "creator".into(),
                amount: collateral.clone(),
            })
        );

        // check deleted
        let _ = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap_err();
    }

    #[test]
    fn burn() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let counter_offer = coins(40, "ETH");
        let msg = InstantiateMsg {
            counter_offer: counter_offer.clone(),
            expires: 100_000,
        };
        let collateral = coins(1, "BTC");
        let info = mock_info("creator", &collateral);
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(0, res.messages.len());
        let _res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();

        // non-expired cannot burn
        let _info = mock_info("creator", &[]);
        let _msg = ExecuteMsg::Burn {};
        let _err = execute(deps.as_mut(), mock_env(), _info, _msg).unwrap_err();
        match _err {
            ContractError::CustomError { val } => assert_eq!("Option not yet expired", val),
            e => panic!("unexpected error: {}", e),
        }

        // do not send funds to burn
        let _info = mock_info("creator", &coins(136, "ADA"));
        let _msg = ExecuteMsg::Burn {};
        let mut env = mock_env();
        env.block.height = 200_000;
        let _err = execute(deps.as_mut(), env, _info, _msg).unwrap_err();
        match _err {
            ContractError::CustomError { val } => assert_eq!("dont send funds with burn", val),
            e => panic!("unexpected error: {}", e),
        }

        // return funds if expired
        let _info = mock_info("creator", &[]);
        let _msg = ExecuteMsg::Burn {};
        let mut _env = mock_env();
        _env.block.height = 200_000;
        let success = execute(deps.as_mut(), _env, _info, _msg).unwrap();
        assert_eq!(success.messages.len(), 1);
        assert_eq!(success.attributes.len(), 1);
        assert_eq!(
            success.messages[0],
            SubMsg::new(BankMsg::Send {
                to_address: "creator".into(),
                amount: collateral.clone(),
            })
        );
    }
}
