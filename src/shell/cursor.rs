//! Pointer shapes applications name (cursor-shapes research C3): the shapes of the cursor shape
//! protocol as plain names, with no Wayland types (Principle IV).

/// A shape of the cursor shape protocol, in protocol order (values 1 to 36).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorName {
    Default,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    EResize,
    NResize,
    NeResize,
    NwResize,
    SResize,
    SeResize,
    SwResize,
    WResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
    AllScroll,
    ZoomIn,
    ZoomOut,
    DndAsk,
    AllResize,
}

impl CursorName {
    /// Every shape, in protocol order: `ALL[v - 1]` is the shape with protocol value `v`.
    pub const ALL: [CursorName; 36] = [
        CursorName::Default,
        CursorName::ContextMenu,
        CursorName::Help,
        CursorName::Pointer,
        CursorName::Progress,
        CursorName::Wait,
        CursorName::Cell,
        CursorName::Crosshair,
        CursorName::Text,
        CursorName::VerticalText,
        CursorName::Alias,
        CursorName::Copy,
        CursorName::Move,
        CursorName::NoDrop,
        CursorName::NotAllowed,
        CursorName::Grab,
        CursorName::Grabbing,
        CursorName::EResize,
        CursorName::NResize,
        CursorName::NeResize,
        CursorName::NwResize,
        CursorName::SResize,
        CursorName::SeResize,
        CursorName::SwResize,
        CursorName::WResize,
        CursorName::EwResize,
        CursorName::NsResize,
        CursorName::NeswResize,
        CursorName::NwseResize,
        CursorName::ColResize,
        CursorName::RowResize,
        CursorName::AllScroll,
        CursorName::ZoomIn,
        CursorName::ZoomOut,
        CursorName::DndAsk,
        CursorName::AllResize,
    ];

    /// The CSS name, used in log lines (`e-resize`, `dnd-ask`, ...).
    pub fn css_name(self) -> &'static str {
        match self {
            CursorName::Default => "default",
            CursorName::ContextMenu => "context-menu",
            CursorName::Help => "help",
            CursorName::Pointer => "pointer",
            CursorName::Progress => "progress",
            CursorName::Wait => "wait",
            CursorName::Cell => "cell",
            CursorName::Crosshair => "crosshair",
            CursorName::Text => "text",
            CursorName::VerticalText => "vertical-text",
            CursorName::Alias => "alias",
            CursorName::Copy => "copy",
            CursorName::Move => "move",
            CursorName::NoDrop => "no-drop",
            CursorName::NotAllowed => "not-allowed",
            CursorName::Grab => "grab",
            CursorName::Grabbing => "grabbing",
            CursorName::EResize => "e-resize",
            CursorName::NResize => "n-resize",
            CursorName::NeResize => "ne-resize",
            CursorName::NwResize => "nw-resize",
            CursorName::SResize => "s-resize",
            CursorName::SeResize => "se-resize",
            CursorName::SwResize => "sw-resize",
            CursorName::WResize => "w-resize",
            CursorName::EwResize => "ew-resize",
            CursorName::NsResize => "ns-resize",
            CursorName::NeswResize => "nesw-resize",
            CursorName::NwseResize => "nwse-resize",
            CursorName::ColResize => "col-resize",
            CursorName::RowResize => "row-resize",
            CursorName::AllScroll => "all-scroll",
            CursorName::ZoomIn => "zoom-in",
            CursorName::ZoomOut => "zoom-out",
            CursorName::DndAsk => "dnd-ask",
            CursorName::AllResize => "all-resize",
        }
    }
}

/// What an application asked for since its latest pointer enter (research C2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCursor {
    /// A named shape; a cursor image of its own and the reset at each enter are `Default`.
    Shape(CursorName),
    /// No pointer over its windows.
    Hidden,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_the_36_shapes_in_protocol_order() {
        assert_eq!(CursorName::ALL.len(), 36);
        assert_eq!(CursorName::ALL[0], CursorName::Default);
        assert_eq!(CursorName::ALL[8], CursorName::Text, "text is 9");
        assert_eq!(CursorName::ALL[17], CursorName::EResize, "e_resize is 18");
        assert_eq!(
            CursorName::ALL[29],
            CursorName::ColResize,
            "col_resize is 30"
        );
        assert_eq!(CursorName::ALL[34], CursorName::DndAsk, "dnd_ask is 35");
        assert_eq!(
            CursorName::ALL[35],
            CursorName::AllResize,
            "all_resize is 36"
        );
        for (i, a) in CursorName::ALL.iter().enumerate() {
            assert!(!CursorName::ALL[..i].contains(a), "{a:?} twice");
        }
    }

    #[test]
    fn css_names_are_the_css_ones_and_unique() {
        let expected = [
            "default",
            "context-menu",
            "help",
            "pointer",
            "progress",
            "wait",
            "cell",
            "crosshair",
            "text",
            "vertical-text",
            "alias",
            "copy",
            "move",
            "no-drop",
            "not-allowed",
            "grab",
            "grabbing",
            "e-resize",
            "n-resize",
            "ne-resize",
            "nw-resize",
            "s-resize",
            "se-resize",
            "sw-resize",
            "w-resize",
            "ew-resize",
            "ns-resize",
            "nesw-resize",
            "nwse-resize",
            "col-resize",
            "row-resize",
            "all-scroll",
            "zoom-in",
            "zoom-out",
            "dnd-ask",
            "all-resize",
        ];
        for (name, css) in CursorName::ALL.iter().zip(expected) {
            assert_eq!(name.css_name(), css, "{name:?}");
        }
    }
}
