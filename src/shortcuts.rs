//! Shared, read-only help catalog for the CLI and the in-editor help overlay.

pub(crate) struct Section {
    pub title: &'static str,
    pub bindings: &'static [(&'static str, &'static str)],
}

pub(crate) const SECTIONS: &[Section] = &[
    Section {
        title: "Help",
        bindings: &[
            (
                "F1",
                "Open help; press F1 or Esc to return to the previous context.",
            ),
            ("Up / Down", "Scroll help one row."),
            ("PageUp / PageDown", "Scroll help one page."),
            ("Home / End", "Go to the beginning or end of help."),
            ("Mouse wheel", "Scroll help without changing the document."),
        ],
    },
    Section {
        title: "Files and documents",
        bindings: &[
            ("Ctrl+N", "New tab in the active editor pane."),
            ("Ctrl+O", "Open a path in the last active editor pane."),
            (
                "Ctrl+S",
                "Save the active document; ask for a path if needed.",
            ),
            ("F4", "Save As. Other open documents cannot be overwritten."),
            (
                "Ctrl+W",
                "Close this view; protect changes on the last view.",
            ),
            (
                "Ctrl+Q",
                "Save the session and quit; separate editing checks dirty documents.",
            ),
        ],
    },
    Section {
        title: "Editing",
        bindings: &[
            ("Arrows", "Move by grapheme or display row."),
            ("Ctrl+Left/Right", "Move by word."),
            ("Home / End", "Move to the beginning or end of the line."),
            (
                "Ctrl+Home/End",
                "Move to the beginning or end of the document.",
            ),
            ("PageUp / PageDown", "Move by one viewport."),
            ("Shift+movement", "Extend the selection."),
            ("Ctrl+A", "Select all text."),
            (
                "Ctrl+C / X / V",
                "Copy, cut or paste using the selected clipboard backend.",
            ),
            (
                "Ctrl+Z / Ctrl+Y",
                "Undo or redo the shared document history.",
            ),
            (
                "Tab",
                "Insert to the next four-column stop, or indent a multiline selection.",
            ),
            (
                "Shift+Tab",
                "Dedent the current or selected lines: one leading tab or up to four spaces.",
            ),
            (
                "Backspace / Delete",
                "Delete the previous or next grapheme.",
            ),
            ("Click / double-click", "Place the cursor or select a word."),
            (
                "Drag / wheel",
                "Select with autoscroll or scroll the area under the pointer.",
            ),
        ],
    },
    Section {
        title: "Find and Replace",
        bindings: &[
            (
                "Ctrl+F / Ctrl+R",
                "Open Find or Replace; single-line editor selection up to 4096 bytes prefills Find.",
            ),
            (
                "Enter / F3",
                "Find the next literal match, wrapping once and marking (wrapped).",
            ),
            (
                "Shift+F3",
                "Find the previous match, wrapping once and marking (wrapped).",
            ),
            (
                "Match count",
                "Shows current position and total up to 100+ (>100 if past 100); navigation stays uncapped.",
            ),
            ("F6", "In Replace: replace the validated current match."),
            (
                "F7",
                "In Replace: replace all; one undo restores the operation.",
            ),
            ("Tab / Shift+Tab", "Switch Find and Replacement fields."),
            ("Esc", "Close the dialog; F3 can repeat the last query."),
            (
                "Matching",
                "Literal and case-sensitive, not regular expressions.",
            ),
        ],
    },
    Section {
        title: "Panes and tabs",
        bindings: &[
            (
                "F8 / F9",
                "Split left/right or top/bottom, sharing the active document.",
            ),
            (
                "F10 / Shift+F10",
                "Focus the next or previous pane, including hidden panes.",
            ),
            (
                "F12",
                "Close the pane; protect documents losing their final view.",
            ),
            ("Alt+Left/Right", "Switch tabs within the active pane."),
            (
                "Ctrl+PageUp/Down",
                "Alternative tab switching within the active pane.",
            ),
            (
                "Alt+Shift+Left/Right",
                "Resize the explorer by two columns when focused, or the nearest left/right split when editing.",
            ),
            (
                "Alt+Shift+Up/Down",
                "Move the nearest top/bottom divider by one row.",
            ),
            (
                "Divider drag",
                "Resize a pane or the explorer within layout minimums.",
            ),
            (
                "Scrollbar track / thumb",
                "Page or drag the editor, explorer, or Help viewport without changing selection.",
            ),
            (
                "Small terminals",
                "Hide panes temporarily; restore the stored layout on resize.",
            ),
        ],
    },
    Section {
        title: "Explorer",
        bindings: &[
            ("Ctrl+B", "Show or hide the explorer."),
            (
                "F2 / Esc",
                "Switch explorer/editor focus, or return to the editor.",
            ),
            ("Up/Down/Home/End", "Select a filesystem entry."),
            (
                "PageUp / PageDown",
                "Move the explorer selection by a page.",
            ),
            (
                "Right / Left",
                "Expand a directory or collapse/select its parent.",
            ),
            (
                "Enter",
                "Browse into a directory or open the selected file.",
            ),
            ("Backspace", "Browse the parent directory; disabled at /."),
            (
                "Click",
                "Open a file, toggle a directory, or use the parent row.",
            ),
            ("F5", "Refresh the tree or retry an unavailable explorer."),
            (
                "Alt+Shift+Left/Right",
                "Resize the explorer by two columns while focused and visible.",
            ),
        ],
    },
    Section {
        title: "Appearance",
        bindings: &[
            (
                "F11 / Alt+P",
                "Open from editor or explorer, not over another modal.",
            ),
            (
                "Tab / Shift+Tab / Up / Down",
                "Move focus among theme, colors, icons, Apply and Cancel.",
            ),
            (
                "Left / Right",
                "Change a row value or move between buttons.",
            ),
            (
                "Space",
                "Cycle the focused row or activate the focused button.",
            ),
            ("Enter / A", "Apply; Enter on Cancel cancels instead."),
            (
                "t/T / c/C / i/I",
                "Select theme, colors or icons and cycle next/previous.",
            ),
            (
                "Esc / F1",
                "Cancel the draft or open Help; F1 returns to the draft and focus.",
            ),
            (
                "Apply",
                "Changes are run-only, without preview or configuration writes; terminal font is unchanged and Nerd icons are opt-in.",
            ),
        ],
    },
    Section {
        title: "Confirmation dialogs",
        bindings: &[
            (
                "Tab / Right / Down",
                "Move button focus forward (starts on Cancel).",
            ),
            (
                "Shift+Tab / Left / Up",
                "Move button focus backward, wrapping around.",
            ),
            ("Enter / Space", "Activate the focused confirmation button."),
            (
                "Letters / Esc / F1",
                "Use shown button mnemonics, cancel with Esc, or open Help with F1.",
            ),
        ],
    },
    Section {
        title: "Dialog fields",
        bindings: &[
            (
                "Left/Right/Home/End",
                "Move the field caret without changing the document.",
            ),
            ("Ctrl+Left/Right", "Move the field caret by word."),
            ("Shift+movement", "Select text in the focused field."),
            ("Ctrl+A / Ctrl+U", "Select the field or clear it."),
            (
                "Backspace / Delete",
                "Delete a grapheme or the field selection.",
            ),
            (
                "Ctrl+C / X / V",
                "Use the selected clipboard backend for the focused field only.",
            ),
            (
                "Click / drag",
                "Place the field caret or extend its selection.",
            ),
            (
                "Enter / Esc",
                "Accept or cancel; in Find, Enter finds the next match.",
            ),
            (
                "Field limit",
                "4096 UTF-8 bytes. Invalid or oversized input is rejected whole.",
            ),
        ],
    },
    Section {
        title: "Safety and terminal limits",
        bindings: &[
            (
                "Save / Discard / Cancel",
                "Decisions apply to one document, never silently to all.",
            ),
            (
                "Clipboard",
                "Auto uses local wl-clipboard/xclip when available; otherwise internal. OSC 52 is not enabled.",
            ),
            (
                "Terminal paste",
                "Bracketed paste up to 1 MiB; control bytes remain data.",
            ),
            (
                "Ctrl+H",
                "Backspace on terminals, not Replace. Use Ctrl+R for Replace.",
            ),
            (
                "Session",
                "Plain naf restores buffers on launch and saves them on normal quit; not encrypted.",
            ),
            (
                "--no-session",
                "Separate editing without snapshots; file/folder arguments also stay separate.",
            ),
            (
                "Recovery",
                "No autosave or crash recovery; save explicitly.",
            ),
        ],
    },
];
