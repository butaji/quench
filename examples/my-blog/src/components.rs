// Components module
//! Static components that render on the server only (no client JS)

/// Header component props
pub mod header {
    #[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct HeaderProps {
        pub title: String,
        pub subtitle: String,
    }
}
