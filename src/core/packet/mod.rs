#[cfg(feature = "fuso-serde")]
mod serde;
#[cfg(feature = "fuso-serde")]
pub use self::serde::*;

#[cfg(not(feature = "fuso-serde"))]
mod local;
#[cfg(not(feature = "fuso-serde"))]
pub use self::local::*;

mod stream;

pub use self::stream::*;