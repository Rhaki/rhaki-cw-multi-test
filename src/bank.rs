use cosmwasm_std::{
    coin, to_binary, Addr, AllBalanceResponse, BalanceResponse, BankMsg, BankQuery, Binary, Coin,
    Storage,
};

use cw0::NativeBalance;
use cw_storage_plus::Map;

const BALANCES: Map<&Addr, NativeBalance> = Map::new("balances");

/// Bank is a minimal contract-like interface that implements a bank module
/// It is initialized outside of the trait
pub trait Bank {
    fn execute(&self, storage: &mut dyn Storage, sender: Addr, msg: BankMsg) -> Result<(), String>;

    fn query(&self, storage: &dyn Storage, request: BankQuery) -> Result<Binary, String>;

    // TODO: how to make "generic" admin extensions?
    // App includes `pub bank: C <C: Bank>`, and just calls this directly
    // Also `pub fn with_state_cache(Fn<(BlockInfo, &mut Storage) -> Result<T, String>) -> Result<T, String>`
    // App.storage is not public, but you can get a cache to write to (with admin methods)
    // this is an "admin" function to let us adjust bank accounts
    fn set_balance(
        &self,
        storage: &mut dyn Storage,
        account: &Addr,
        amount: Vec<Coin>,
    ) -> Result<(), String>;

    // TODO: remove need for clone
    fn clone(&self) -> Box<dyn Bank>;
}

#[derive(Default)]
pub struct SimpleBank {}

impl SimpleBank {
    // this is an "admin" function to let us adjust bank accounts
    pub fn get_balance(&self, storage: &dyn Storage, account: &Addr) -> Result<Vec<Coin>, String> {
        let val = BALANCES
            .may_load(storage, &account)
            .map_err(|e| e.to_string())?;
        Ok(val.unwrap_or_default().into_vec())
    }

    fn send(
        &self,
        storage: &mut dyn Storage,
        from_address: Addr,
        to_address: Addr,
        amount: Vec<Coin>,
    ) -> Result<(), String> {
        self.burn(storage, from_address, amount.clone())?;
        self.mint(storage, to_address, amount)
    }

    fn mint(
        &self,
        storage: &mut dyn Storage,
        to_address: Addr,
        amount: Vec<Coin>,
    ) -> Result<(), String> {
        let b = self.get_balance(storage, &to_address)?;
        let b = NativeBalance(b) + NativeBalance(amount);
        self.set_balance(storage, &to_address, b.into_vec())
    }

    fn burn(
        &self,
        storage: &mut dyn Storage,
        from_address: Addr,
        amount: Vec<Coin>,
    ) -> Result<(), String> {
        let a = self.get_balance(storage, &from_address)?;
        let a = (NativeBalance(a) - amount).map_err(|e| e.to_string())?;
        self.set_balance(storage, &from_address, a.into_vec())
    }
}

impl Bank for SimpleBank {
    fn execute(&self, storage: &mut dyn Storage, sender: Addr, msg: BankMsg) -> Result<(), String> {
        match msg {
            BankMsg::Send { to_address, amount } => {
                self.send(storage, sender, Addr::unchecked(to_address), amount)
            }
            BankMsg::Burn { amount } => self.burn(storage, sender, amount),
            m => panic!("Unsupported bank message: {:?}", m),
        }
    }

    fn query(&self, storage: &dyn Storage, request: BankQuery) -> Result<Binary, String> {
        match request {
            BankQuery::AllBalances { address } => {
                // TODO: shall we pass in Api to make this safer?
                let amount = self.get_balance(storage, &Addr::unchecked(address))?;
                let res = AllBalanceResponse { amount };
                Ok(to_binary(&res).map_err(|e| e.to_string())?)
            }
            BankQuery::Balance { address, denom } => {
                // TODO: shall we pass in Api to make this safer?
                let all_amounts = self.get_balance(storage, &Addr::unchecked(address))?;
                let amount = all_amounts
                    .into_iter()
                    .find(|c| c.denom == denom)
                    .unwrap_or_else(|| coin(0, denom));
                let res = BalanceResponse { amount };
                Ok(to_binary(&res).map_err(|e| e.to_string())?)
            }
            q => panic!("Unsupported bank query: {:?}", q),
        }
    }

    // this is an "admin" function to let us adjust bank accounts
    fn set_balance(
        &self,
        storage: &mut dyn Storage,
        account: &Addr,
        amount: Vec<Coin>,
    ) -> Result<(), String> {
        let mut balance = NativeBalance(amount);
        balance.normalize();
        BALANCES
            .save(storage, account, &balance)
            .map_err(|e| e.to_string())
    }

    fn clone(&self) -> Box<dyn Bank> {
        Box::new(SimpleBank {})
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use cosmwasm_std::testing::MockStorage;
    use cosmwasm_std::{coins, from_slice};

    #[test]
    fn get_set_balance() {
        let mut store = MockStorage::new();

        let owner = Addr::unchecked("owner");
        let rcpt = Addr::unchecked("receiver");
        let init_funds = vec![coin(100, "eth"), coin(20, "btc")];
        let norm = vec![coin(20, "btc"), coin(100, "eth")];

        // set money
        let bank = SimpleBank {};
        bank.set_balance(&mut store, &owner, init_funds).unwrap();

        // get balance work
        let rich = bank.get_balance(&store, &owner).unwrap();
        assert_eq!(rich, norm);
        let poor = bank.get_balance(&store, &rcpt).unwrap();
        assert_eq!(poor, vec![]);

        // proper queries work
        let req = BankQuery::AllBalances {
            address: owner.clone().into(),
        };
        let raw = bank.query(&store, req).unwrap();
        let res: AllBalanceResponse = from_slice(&raw).unwrap();
        assert_eq!(res.amount, norm);

        let req = BankQuery::AllBalances {
            address: rcpt.clone().into(),
        };
        let raw = bank.query(&store, req).unwrap();
        let res: AllBalanceResponse = from_slice(&raw).unwrap();
        assert_eq!(res.amount, vec![]);

        let req = BankQuery::Balance {
            address: owner.clone().into(),
            denom: "eth".into(),
        };
        let raw = bank.query(&store, req).unwrap();
        let res: BalanceResponse = from_slice(&raw).unwrap();
        assert_eq!(res.amount, coin(100, "eth"));

        let req = BankQuery::Balance {
            address: owner.into(),
            denom: "foobar".into(),
        };
        let raw = bank.query(&store, req).unwrap();
        let res: BalanceResponse = from_slice(&raw).unwrap();
        assert_eq!(res.amount, coin(0, "foobar"));

        let req = BankQuery::Balance {
            address: rcpt.into(),
            denom: "eth".into(),
        };
        let raw = bank.query(&store, req).unwrap();
        let res: BalanceResponse = from_slice(&raw).unwrap();
        assert_eq!(res.amount, coin(0, "eth"));
    }

    #[test]
    fn send_coins() {
        let mut store = MockStorage::new();

        let owner = Addr::unchecked("owner");
        let rcpt = Addr::unchecked("receiver");
        let init_funds = vec![coin(20, "btc"), coin(100, "eth")];
        let rcpt_funds = vec![coin(5, "btc")];

        // set money
        let bank = SimpleBank {};
        bank.set_balance(&mut store, &owner, init_funds).unwrap();
        bank.set_balance(&mut store, &rcpt, rcpt_funds).unwrap();

        // send both tokens
        let to_send = vec![coin(30, "eth"), coin(5, "btc")];
        let msg = BankMsg::Send {
            to_address: rcpt.clone().into(),
            amount: to_send,
        };
        bank.execute(&mut store, owner.clone(), msg.clone())
            .unwrap();
        let rich = bank.get_balance(&store, &owner).unwrap();
        assert_eq!(vec![coin(15, "btc"), coin(70, "eth")], rich);
        let poor = bank.get_balance(&store, &rcpt).unwrap();
        assert_eq!(vec![coin(10, "btc"), coin(30, "eth")], poor);

        // can send from any account with funds
        bank.execute(&mut store, rcpt.clone(), msg).unwrap();

        // cannot send too much
        let msg = BankMsg::Send {
            to_address: rcpt.into(),
            amount: coins(20, "btc"),
        };
        bank.execute(&mut store, owner.clone(), msg).unwrap_err();

        let rich = bank.get_balance(&store, &owner).unwrap();
        assert_eq!(vec![coin(15, "btc"), coin(70, "eth")], rich);
    }

    #[test]
    fn burn_coins() {
        let mut store = MockStorage::new();

        let owner = Addr::unchecked("owner");
        let rcpt = Addr::unchecked("recipient");
        let init_funds = vec![coin(20, "btc"), coin(100, "eth")];

        // set money
        let bank = SimpleBank {};
        bank.set_balance(&mut store, &owner, init_funds).unwrap();

        // burn both tokens
        let to_burn = vec![coin(30, "eth"), coin(5, "btc")];
        let msg = BankMsg::Burn { amount: to_burn };
        bank.execute(&mut store, owner.clone(), msg).unwrap();
        let rich = bank.get_balance(&store, &owner).unwrap();
        assert_eq!(vec![coin(15, "btc"), coin(70, "eth")], rich);

        // cannot burn too much
        let msg = BankMsg::Burn {
            amount: coins(20, "btc"),
        };
        let err = bank.execute(&mut store, owner.clone(), msg).unwrap_err();
        assert!(err.contains("Overflow"));
        let rich = bank.get_balance(&store, &owner).unwrap();
        assert_eq!(vec![coin(15, "btc"), coin(70, "eth")], rich);

        // cannot burn from empty account
        let msg = BankMsg::Burn {
            amount: coins(1, "btc"),
        };
        let err = bank.execute(&mut store, rcpt, msg).unwrap_err();
        assert!(err.contains("Overflow"));
    }
}
