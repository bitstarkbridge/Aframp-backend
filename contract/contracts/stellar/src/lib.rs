#![no_std]

mod storage;
mod events;
mod error;

use soroban_sdk::{contract, contractimpl, Env, Address};

#[contract]
pub struct AfrIContract;

#[contractimpl]
impl AfrIContract {
    pub fn init(env: Env, admin: Address) {
        storage::set_admin(&env, &admin);
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        let caller = env.invoker();
        let admin = storage::get_admin(&env);
        if caller != admin {
            panic!("Only admin can mint");
        }
        storage::set_balance(&env, &to, storage::get_balance(&env, &to) + amount);
    }

    pub fn burn(env: Env, from: Address, amount: i128) {
        let current = storage::get_balance(&env, &from);
        if amount > current {
            panic!("Insufficient balance to burn");
        }
        storage::set_balance(&env, &from, current - amount);
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let from_balance = storage::get_balance(&env, &from);
        if amount > from_balance {
            panic!("Insufficient balance to transfer");
        }
        storage::set_balance(&env, &from, from_balance - amount);
        storage::set_balance(&env, &to, storage::get_balance(&env, &to) + amount);
    }

    pub fn balance(env: Env, user: Address) -> i128 {
        storage::get_balance(&env, &user)
    }
}

mod test;
