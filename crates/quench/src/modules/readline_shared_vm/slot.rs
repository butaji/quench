//! The private slots of a readline interface.

/// Declares the slots and their symbols' descriptions, one row per slot.
macro_rules! slots {
    ($(#[$meta:meta])* $($slot:ident => $description:literal),+ $(,)?) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(super) enum Slot { $($slot),+ }

        /// Each slot's description, in declaration order.
        const DESCRIPTIONS: [&str; [$($description),+].len()] = [$($description),+];
    };
}

slots! {
    /// The symbols an interface keys its private state and internal methods with.
    SawReturnAt => "_sawReturnAt",
    SawKeyPress => "_sawKeyPress",
    PreviousKey => "_previousKey",
    SubstringSearch => "kSubstringSearch",
    UndoStack => "_undoStack",
    RedoStack => "_redoStack",
    KillRing => "_killRing",
    KillRingCursor => "_killRingCursor",
    Prompt => "_prompt",
    LineBuffer => "_line_buffer",
    Decoder => "_decoder",
    LineObjectStream => "line object stream",
    QuestionCallback => "_questionCallback",
    OldPrompt => "_oldPrompt",
    QuestionReject => "_questionReject",
    Question => "kQuestion",
    QuestionCancel => "kQuestionCancel",
    SetRawMode => "_setRawMode",
    OnLine => "_onLine",
    WriteToOutput => "_writeToOutput",
    AddHistory => "_addHistory",
    RefreshLine => "_refreshLine",
    NormalWrite => "_normalWrite",
    InsertString => "_insertString",
    Line => "_line",
    HistoryNext => "_historyNext",
    HistoryPrev => "_historyPrev",
    GetDisplayPos => "_getDisplayPos",
    MoveCursor => "_moveCursor",
    TtyWrite => "_ttyWrite",
    DeleteLeft => "_deleteLeft",
    DeleteRight => "_deleteRight",
    DeleteLineLeft => "_deleteLineLeft",
    DeleteLineRight => "_deleteLineRight",
    KeypressDecoder => "keypress-decoder",
    EscapeDecoder => "escape-decoder",
}

impl Slot {
    /// Whether the slot belongs to the `question` protocol.
    pub(super) fn is_question(self) -> bool {
        matches!(self, Slot::Question | Slot::QuestionCancel)
    }

    /// The symbol's description, which for the legacy-aliased slots is also the alias.
    pub(super) fn description(self) -> &'static str {
        DESCRIPTIONS[self as usize]
    }

    pub(super) fn cache_key(self) -> String {
        format!("\0readline:{}", self.description())
    }
}
