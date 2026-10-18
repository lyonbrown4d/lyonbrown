//! Native desktop execution for an authenticated interactive-user helper.
//!
//! The host owns approval, durable intent/receipt storage and helper authentication.
//! Share one [`LeaseRegistry`] per helper; never expose [`AuthorizedAction::new`]
//! to a model or deserialize it from a tool request. OS permissions are inspected,
//! never requested or changed by this library.

mod action;
mod backend;
mod content;
mod lease;
#[cfg(feature = "native")]
pub mod native;
mod observation;
mod prepared;
mod runtime;
mod types;

pub use action::*;
pub use backend::*;
pub use lease::*;
pub use observation::*;
pub use prepared::PreparedDesktopAction;
pub use runtime::*;
pub use types::*;
