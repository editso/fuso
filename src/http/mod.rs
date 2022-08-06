#[cfg(feature = "fuso-api")]
pub mod routes;

#[cfg(all(feature = "fuso-api", feature = "fuso-web"))]
pub mod pages;
