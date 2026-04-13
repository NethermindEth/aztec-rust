# Authorization

Normative rules for authorization witnesses (authwits) and account authentication.

## Authwit Binding

**AUTH-1.** An `AuthWitness` MUST bind to a specific `(from, intent, chain_info)` triple.
Implementations that consume a witness MUST verify all three before acting on it.

**AUTH-2.** A `MessageHashOrIntent` carried inside an authwit MUST be derivable from the call it authorizes; witnesses constructed against a different intent MUST be rejected at consumption.

**AUTH-3.** An `AccountProvider::create_auth_wit` implementation MUST NOT sign an intent it cannot verify against the declared `from` address.

## Entrypoints

**AUTH-4.** An account entrypoint MUST validate the nonce contained in the incoming `EncodedAppEntrypointCalls` and MUST reject replayed nonces.

**AUTH-5.** An entrypoint that dispatches multiple calls (`DefaultMultiCallEntrypoint`) MUST apply the authorization check atomically; either all calls are authorized in the same tx, or none are.

**AUTH-6.** An entrypoint MUST reject any call where the inner `msg_sender` claim does not derive from the account's address.

## Public Authwits

**AUTH-7.** `SetPublicAuthWitInteraction` MUST target the AuthRegistry protocol contract (`protocol_contract_address::auth_registry`). Targeting a different address is a correctness bug.

**AUTH-8.** Consumption of a public authwit MUST consume the associated `authwit` nullifier; implementations MUST NOT treat a consumed authwit as re-usable.

**AUTH-9.** `lookup_validity` results are advisory for a specific chain head; applications SHOULD NOT cache them beyond the observing block.

## Signerless Accounts

**AUTH-10.** `SignerlessAccount` MUST NOT be used outside of account deployment bootstrap or tests.
An implementation that allows a `SignerlessAccount` to sign user-originated transactions is a vulnerability.

## Authwit Construction

**AUTH-11.** When a wallet constructs an authwit on behalf of a user for a call they did not sign inline, it MUST surface the intent (e.g. `MessageHashOrIntent::Intent`) to the user before signing.

**AUTH-12.** Applications SHOULD NOT request blanket authwits (authwits bound to an overly broad intent); authwits MUST be as narrow as the target contract's semantics allow.
