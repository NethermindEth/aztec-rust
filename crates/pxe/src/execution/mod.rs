//! Private function execution and oracle handling.

pub mod acvm_executor;
pub mod oracle;
pub mod utility_oracle;

pub use acvm_executor::AcvmExecutor;
pub use oracle::PrivateExecutionOracle;
pub use utility_oracle::UtilityExecutionOracle;
