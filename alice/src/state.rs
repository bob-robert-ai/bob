use crate::ics_pool::PublicPoolOverView;
use crate::{TaskType, Token};
use candid::{Deserialize, Principal};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use strum::IntoEnumIterator;

pub struct PriceTracker {
    prices: VecDeque<PublicPoolOverView>,
    max_size: usize,
}

impl PriceTracker {
    pub fn new(max_size: usize) -> Self {
        Self {
            prices: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    pub fn add_price(&mut self, price: PublicPoolOverView) {
        if self.prices.len() == self.max_size {
            self.prices.pop_front();
        }
        self.prices.push_back(price);
    }

    pub fn get_prices(&self) -> Vec<PublicPoolOverView> {
        self.prices
            .iter()
            .cloned()
            .collect::<Vec<PublicPoolOverView>>()
    }

    pub fn get_latest(&self) -> Option<PublicPoolOverView> {
        self.prices.back().cloned()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Quote {
    pub value: u64,
    pub ts: u64,
}

pub struct State {
    pub balances: BTreeMap<Token, u64>,
    pub prices: BTreeMap<Token, PriceTracker>,

    pub principal_guards: BTreeSet<Principal>,
    pub active_tasks: BTreeSet<TaskType>,
}

impl State {
    pub fn new() -> State {
        State {
            balances: Default::default(),
            prices: BTreeMap::from([
                (Token::Bob, PriceTracker::new(8_usize)),
                (Token::Alice, PriceTracker::new(8_usize)),
            ]),
            principal_guards: Default::default(),
            active_tasks: Default::default(),
        }
    }

    pub fn insert_price(&mut self, token: Token, price: PublicPoolOverView) {
        if let Some(price_tracker) = self.prices.get_mut(&token) {
            price_tracker.add_price(price);
        }
    }

    pub fn get_all_prices(&self) -> String {
        let mut result = String::new();

        for (token, price_tracker) in &self.prices {
            let prices_str = price_tracker
                .get_prices()
                .iter()
                .map(|price| format!("{}", price.display()))
                .collect::<Vec<String>>()
                .join(", ");

            result.push_str(&format!(" - {}: [{}]\n", token, prices_str));
        }

        result
    }

    pub fn get_balance(&self, token: Token) -> u64 {
        self.balances.get(&token).unwrap_or(&0).clone()
    }

    pub fn maybe_get_asset_value_in_portfolio(&self, token: Token) -> Option<u64> {
        if token == Token::Icp {
            return Some(read_state(|s| s.get_balance(token)));
        }
        crate::memory::maybe_get_last_quote(token)
            .map(|quote| self.get_balance(token) * quote.value / 100_000_000)
    }

    pub fn maybe_portfolio_value(&self) -> Option<u64> {
        let mut res = 0;
        for token in Token::iter() {
            if let Some(value) = self.maybe_get_asset_value_in_portfolio(token) {
                res += value;
            } else {
                return None;
            }
        }
        Some(res)
    }

    pub fn compute_token_returns(&self, token: Token) -> Vec<f64> {
        let mut result = vec![];
        let data: Vec<Quote> = crate::memory::get_quotes(token);

        let quotes: Vec<&Quote> = data.iter().collect::<Vec<&Quote>>();

        for i in 1..quotes.len() {
            let curr = quotes[i].value.clone() as f64;
            let previous = quotes[i - 1].value.clone() as f64;
            let pct_change = (curr - previous) / previous;
            result.push(pct_change);
        }
        result
    }

    pub fn get_value_at_risk(&self, token: Token) -> f64 {
        if token == Token::Icp {
            return 0.0;
        }
        let returns = self.compute_token_returns(token);
        let mut losses: Vec<f64> = returns.iter().map(|r| (-r).max(0.0)).collect();
        losses.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let var_index = (0.95 * losses.len() as f64) as usize;

        if losses.len() == 0 {
            return 0.0;
        }
        losses[var_index]
    }

    pub fn var_majorant(&self, portfolio_value: u64, asset_value: u64) -> f64 {
        let mut result: f64 = 0.0;
        for token in Token::iter() {
            if token == Token::Icp {
                continue;
            }
            let weight: f64 = asset_value as f64 / portfolio_value as f64;
            result += weight * self.get_value_at_risk(token);
        }
        result.min(0.1)
    }

    pub fn amount_to_buy(&self, token: Token) -> u64 {
        let default_trade = self.get_balance(Token::Icp) / 20;
        if let Some(portfolio_value) = self.maybe_portfolio_value() {
            let quote = crate::memory::maybe_get_last_quote(token)
                .map(|q| q.value)
                .unwrap_or(0);
            if quote <= 0 {
                return default_trade;
            }

            let var_maj = self.var_majorant(
                portfolio_value,
                quote * self.get_balance(token) / 100_000_000,
            );
            let var = self.get_value_at_risk(token);

            let max_valid_amount = (100_000_000.0 * (0.1 - var_maj) * (portfolio_value as f64)
                / (quote as f64 * var)) as u64;

            return default_trade.min(max_valid_amount);
        } else {
            return default_trade;
        }
    }
}

thread_local! {
    static __STATE: RefCell<Option<State>> = RefCell::default();
}

pub fn mutate_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    __STATE.with(|s| f(s.borrow_mut().as_mut().expect("State not initialized!")))
}

pub fn read_state<F, R>(f: F) -> R
where
    F: FnOnce(&State) -> R,
{
    __STATE.with(|s| f(s.borrow().as_ref().expect("State not initialized!")))
}

pub fn replace_state(state: State) {
    __STATE.with(|s| {
        *s.borrow_mut() = Some(state);
    });
}
