use std::str::FromStr;

use cosmwasm_std::Uint256;
use ibc_proto::cosmos::{
    bank::v1beta1::{
        MsgSend, QueryAllBalancesRequest, QueryAllBalancesResponse, QueryBalanceRequest,
        QueryBalanceResponse,
    },
    base::v1beta1::Coin,
};

use crate::{error::AppError, store::Store, types::AccAddress};

const BALANCES_PREFIX: [u8; 1] = [2];

#[derive(Debug, Clone)]
pub struct Bank {
    store: Store,
}

pub struct GenesisState {
    pub balances: Vec<Balance>,
}

pub struct Balance {
    pub address: AccAddress,
    pub coins: Vec<Coin>,
}

impl Bank {
    pub fn new(store: Store, genesis: GenesisState) -> Self {
        let bank = Bank { store };

        for balance in genesis.balances {
            let prefix = create_account_balances_prefix(balance.address);
            let account_store = bank.store.get_sub_store(prefix);

            for coin in balance.coins {
                account_store.set(
                    coin.denom.as_bytes().to_vec(),
                    coin.amount.to_string().into(),
                );
            }
        }

        return bank;
    }

    pub fn query_balance(
        &self,
        req: QueryBalanceRequest,
    ) -> Result<QueryBalanceResponse, AppError> {
        let address = AccAddress::from_bech32(&req.address)?;

        let prefix = create_account_balances_prefix(address);

        let account_store = self.store.get_sub_store(prefix);
        let bal = account_store.get(req.denom.as_bytes());

        match bal {
            Some(amount) => Ok(QueryBalanceResponse {
                balance: Some(Coin {
                    denom: req.denom,
                    amount: Uint256::from_str(
                        &String::from_utf8(amount).expect("Should be valid Uint256"),
                    )
                    .expect("Should be valid utf8"),
                }),
            }),
            None => Ok(QueryBalanceResponse { balance: None }),
        }
    }

    pub fn query_all_balances(&self, req: QueryAllBalancesRequest) -> QueryAllBalancesResponse {
        let address = AccAddress::from_bech32(&req.address);

        let address = match address {
            Ok(address) => address,
            Err(_) => {
                return QueryAllBalancesResponse {
                    balances: vec![],
                    pagination: None,
                }
            }
        };
        let prefix = create_account_balances_prefix(address);
        let account_store = self.store.get_sub_store(prefix);

        let mut balances = vec![];

        for (denom, amount) in account_store {
            let denom = String::from_utf8(denom).expect("Should be valid utf8");
            let amount =
                Uint256::from_str(&String::from_utf8(amount).expect("Should be valid Uint256"))
                    .expect("Should be valid utf8");

            let coin = Coin { denom, amount };
            balances.push(coin);
        }

        return QueryAllBalancesResponse {
            balances,
            pagination: None,
        };
    }

    pub fn send_coins(&self, msg: MsgSend) -> Result<(), AppError> {
        let from_account_store = self.get_account_store(&msg.from_address)?;
        let to_account_store = self.get_account_store(&msg.to_address)?;

        for coin in msg.amount {
            let from_balance = from_account_store.get(coin.denom.as_bytes());

            match from_balance {
                None => continue, //TODO: should reject the entire TX
                Some(bal) => {
                    let amount = Uint256::from_str(
                        &String::from_utf8(bal).expect("Should be valid Uint256"),
                    )
                    .expect("Should be valid utf8");

                    if amount < coin.amount {
                        continue; //TODO: should reject the entire TX
                    }

                    from_account_store.set(
                        coin.denom.clone().into(),
                        (amount - coin.amount).to_string().into(),
                    );

                    let to_balance = to_account_store.get(coin.denom.as_bytes());

                    let to_balance = match to_balance {
                        Some(bal) => Uint256::from_str(
                            &String::from_utf8(bal).expect("Should be valid Uint256"),
                        )
                        .expect("Should be valid utf8"),
                        None => Uint256::zero(),
                    };

                    to_account_store.set(
                        coin.denom.into(),
                        (to_balance + coin.amount).to_string().into(),
                    );
                }
            }
        }

        return Ok(());
    }

    fn get_account_store(&self, address: &String) -> Result<Store, AppError> {
        let address = AccAddress::from_bech32(address)?;
        let prefix = create_account_balances_prefix(address);
        Ok(self.store.get_sub_store(prefix))
    }
}

fn create_account_balances_prefix(addr: AccAddress) -> Vec<u8> {
    let addr_len = addr.len();
    let mut addr: Vec<u8> = addr.into();
    let mut prefix = Vec::new();

    prefix.extend(BALANCES_PREFIX);
    prefix.push(addr_len);
    prefix.append(&mut addr);

    return prefix;
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn create_account_balances_prefix_works() {
        let expected = vec![2, 4, 97, 98, 99, 100];
        let acc_address = AccAddress::try_from(vec![97, 98, 99, 100]).unwrap();
        let res = create_account_balances_prefix(acc_address);

        assert_eq!(expected, res);
    }

    #[test]
    fn query_balance_works() {
        let store = Store::new();
        let genesis = GenesisState {
            balances: vec![Balance {
                address: AccAddress::from_bech32("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
                    .unwrap(),
                coins: vec![Coin {
                    denom: "coinA".into(),
                    amount: Uint256::from_str("123").unwrap(),
                }],
            }],
        };

        let bank = Bank::new(store, genesis);

        let req = QueryBalanceRequest {
            address: "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux".to_string(),
            denom: "coinA".to_string(),
        };

        let res = bank.query_balance(req).unwrap();

        let expected_res = QueryBalanceResponse {
            balance: Some(Coin {
                amount: Uint256::from_str("123").unwrap(),
                denom: "coinA".to_string(),
            }),
        };

        assert_eq!(expected_res, res);
    }

    #[test]
    fn query_all_balances_works() {
        let store = Store::new();
        let genesis = GenesisState {
            balances: vec![Balance {
                address: AccAddress::from_bech32("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
                    .unwrap(),
                coins: vec![Coin {
                    denom: "coinA".into(),
                    amount: Uint256::from_str("123").unwrap(),
                }],
            }],
        };

        let bank = Bank::new(store, genesis);

        let req = QueryAllBalancesRequest {
            address: "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux".to_string(),
            pagination: None,
        };

        let res = bank.query_all_balances(req);

        let expected_res = QueryAllBalancesResponse {
            balances: vec![Coin {
                denom: "coinA".to_string(),
                amount: Uint256::from_str("123").unwrap(),
            }],
            pagination: None,
        };

        assert_eq!(expected_res, res);
    }
}