use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchInput {
    Quit,
    Move(isize),
    Enter,
    CycleAgents(isize),
    OpenPicker,
    ToggleRaw,
    Copy,
    Top,
    Bottom,
    StartSearch,
    SearchChar(char),
    SearchBackspace,
    SearchCommit,
    SearchCancel,
}

pub(super) fn map_key(key: KeyEvent, searching: bool) -> Option<WatchInput> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(WatchInput::Quit);
    }
    if searching {
        return match key.code {
            KeyCode::Esc => Some(WatchInput::SearchCancel),
            KeyCode::Enter => Some(WatchInput::SearchCommit),
            KeyCode::Backspace => Some(WatchInput::SearchBackspace),
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                Some(WatchInput::SearchChar(ch))
            }
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char('q') => Some(WatchInput::Quit),
        KeyCode::Up | KeyCode::Char('k') => Some(WatchInput::Move(-1)),
        KeyCode::Down | KeyCode::Char('j') => Some(WatchInput::Move(1)),
        KeyCode::Enter => Some(WatchInput::Enter),
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(WatchInput::CycleAgents(-1))
        }
        KeyCode::Tab => Some(WatchInput::CycleAgents(1)),
        KeyCode::BackTab => Some(WatchInput::CycleAgents(-1)),
        KeyCode::Char('a') => Some(WatchInput::OpenPicker),
        KeyCode::Char('r') => Some(WatchInput::ToggleRaw),
        KeyCode::Char('y') => Some(WatchInput::Copy),
        KeyCode::Char('g') => Some(WatchInput::Top),
        KeyCode::Char('G') => Some(WatchInput::Bottom),
        KeyCode::Char('/') => Some(WatchInput::StartSearch),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{WatchInput, map_key};

    #[test]
    fn maps_required_navigation_and_disclosure_keys() {
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE), false),
            Some(WatchInput::Quit)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), false),
            Some(WatchInput::Move(1))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE), false),
            Some(WatchInput::Move(-1))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false),
            Some(WatchInput::Enter)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), false),
            Some(WatchInput::CycleAgents(1))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT), false),
            Some(WatchInput::CycleAgents(-1))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), false),
            Some(WatchInput::OpenPicker)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE), false),
            Some(WatchInput::ToggleRaw)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), false),
            Some(WatchInput::Copy)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE), false),
            Some(WatchInput::Top)
        );
        assert_eq!(
            map_key(
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                false
            ),
            Some(WatchInput::Bottom)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), false),
            Some(WatchInput::StartSearch)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), true),
            Some(WatchInput::SearchCancel)
        );
    }
}
