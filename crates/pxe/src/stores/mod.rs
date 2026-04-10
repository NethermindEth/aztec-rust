//! Local stores for PXE state.

pub mod address_store;
pub mod capsule_store;
pub mod contract_store;
pub mod key_store;
pub mod kv;
pub mod note_store;
pub mod sender_store;

pub use address_store::AddressStore;
pub use capsule_store::CapsuleStore;
pub use contract_store::ContractStore;
pub use key_store::KeyStore;
pub use kv::{InMemoryKvStore, KvStore};
pub use note_store::NoteStore;
pub use sender_store::SenderStore;
