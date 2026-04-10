//! Private function execution and oracle handling.

pub mod acvm_executor;
pub mod execution_result;
pub mod field_conversion;
pub mod oracle;
pub mod utility_oracle;

pub use acvm_executor::{AcvmExecutor, OracleCallback};
pub use execution_result::{
    PrivateCallExecutionResult, PrivateExecutionResult, PrivateLogData, PublicCallRequestData,
};
pub use field_conversion::{fe_to_fr, fr_to_fe};
pub use oracle::PrivateExecutionOracle;
pub use utility_oracle::UtilityExecutionOracle;
