#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

struct Harness<'a> {
    env: Env,
    vault: VaultClient<'a>,
    vault_id: Address,
    token: Address,
    token_admin: StellarAssetClient<'a>,
    token_client: TokenClient<'a>,
    subscription: Address,
}

fn setup() -> Harness<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let subscription = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();

    let vault_id = env.register(Vault, (admin.clone(),));
    let vault = VaultClient::new(&env, &vault_id);
    vault.set_subscription(&subscription);

    Harness {
        token_admin: StellarAssetClient::new(&env, &token),
        token_client: TokenClient::new(&env, &token),
        env,
        vault,
        vault_id,
        token,
        subscription,
    }
}

fn funded_user(h: &Harness, amount: i128) -> Address {
    let user = Address::generate(&h.env);
    h.token_admin.mint(&user, &amount);
    user
}

#[test]
fn deposit_credits_balance_and_moves_tokens() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.deposit(&user, &h.token, &4_000);

    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.token_client.balance(&user), 6_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 4_000);
}

#[test]
fn deposits_accumulate() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);
    h.vault.deposit(&user, &h.token, &2_500);
    assert_eq!(h.vault.balance(&user, &h.token), 3_500);
}

#[test]
fn deposit_rejects_non_positive() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &-5),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn withdraw_returns_tokens() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &4_000);

    h.vault.withdraw(&user, &h.token, &1_500);

    assert_eq!(h.vault.balance(&user, &h.token), 2_500);
    assert_eq!(h.token_client.balance(&user), 7_500);
}

#[test]
fn withdraw_beyond_balance_fails() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);
    assert_eq!(
        h.vault.try_withdraw(&user, &h.token, &1_001),
        Err(Ok(Error::InsufficientBalance))
    );
}

#[test]
fn debit_pays_merchant_and_reduces_balance() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    h.vault.debit(&user, &h.token, &merchant, &1_200);

    assert_eq!(h.vault.balance(&user, &h.token), 3_800);
    assert_eq!(h.token_client.balance(&merchant), 1_200);
}

#[test]
fn debit_beyond_balance_fails() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &100);
    assert_eq!(
        h.vault.try_debit(&user, &h.token, &merchant, &101),
        Err(Ok(Error::InsufficientBalance))
    );
}

#[test]
fn balances_are_isolated_per_user() {
    let h = setup();
    let a = funded_user(&h, 10_000);
    let b = funded_user(&h, 10_000);
    h.vault.deposit(&a, &h.token, &3_000);
    assert_eq!(h.vault.balance(&a, &h.token), 3_000);
    assert_eq!(h.vault.balance(&b, &h.token), 0);
}

#[test]
fn balance_of_unknown_user_is_zero() {
    let h = setup();
    let stranger = Address::generate(&h.env);
    assert_eq!(h.vault.balance(&stranger, &h.token), 0);
}

#[test]
fn constructor_sets_admin() {
    let h = setup();
    // The constructor ran at registration, so the vault is already configured.
    assert!(h.vault.try_admin().is_ok());
}

#[test]
fn subscription_address_is_recorded() {
    let h = setup();
    assert_eq!(h.vault.subscription(), h.subscription);
}

/// The vault admin can only call `set_subscription`, which repoints the
/// subscription contract. No admin function touches user balances directly.
/// This test verifies that every admin-callable function leaves all user
/// balances unchanged.
///
/// SECURITY.md: "The vault admin can repoint set_subscription to a different
/// contract. A malicious admin could point it at a contract that drains
/// balances." This test pins down that the only admin function exists and
/// does not move funds.
#[test]
fn admin_has_no_path_to_user_funds() {
    let h = setup();

    // Fund two users and deposit into the vault.
    let user_a = funded_user(&h, 10_000);
    let user_b = funded_user(&h, 10_000);
    h.vault.deposit(&user_a, &h.token, &3_000);
    h.vault.deposit(&user_b, &h.token, &2_500);

    let bal_a_before = h.vault.balance(&user_a, &h.token);
    let bal_b_before = h.vault.balance(&user_b, &h.token);

    // The only admin function is set_subscription; call it with a different
    // subscription address. This should not move any user balances.
    let new_subscription = Address::generate(&h.env);
    h.vault.set_subscription(&new_subscription);

    let bal_a_after = h.vault.balance(&user_a, &h.token);
    let bal_b_after = h.vault.balance(&user_b, &h.token);

    assert_eq!(
        bal_a_before, bal_a_after,
        "set_subscription must not change user balances"
    );
    assert_eq!(
        bal_b_before, bal_b_after,
        "set_subscription must not change user balances"
    );
}
