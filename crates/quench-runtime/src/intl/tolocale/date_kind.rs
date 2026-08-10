pub(crate) enum DateLocaleKind {
    String,
    Date,
    Time,
}

impl DateLocaleKind {
    pub(super) fn default(&self) -> &'static str {
        match self {
            Self::String | Self::Date | Self::Time => "Invalid Date",
        }
    }
}
