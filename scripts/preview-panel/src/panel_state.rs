#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentKind {
    Text,
    Image,
    Network,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentItem {
    pub id: u64,
    pub kind: ContentKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwitchDisposition {
    Rejected,
    SameItem,
    Ready,
}

#[derive(Debug, Default)]
pub struct LiveState {
    latest_serial: u64,
    pub current: Option<CurrentItem>,
}

impl LiveState {
    pub fn prepare_switch(&mut self, serial: u64, target_id: u64) -> SwitchDisposition {
        if serial < self.latest_serial {
            return SwitchDisposition::Rejected;
        }
        self.latest_serial = serial;
        if self.current.is_some_and(|item| item.id == target_id) {
            SwitchDisposition::SameItem
        } else {
            SwitchDisposition::Ready
        }
    }

    pub fn apply_update(&mut self, serial: u64, id: u64, kind: ContentKind) -> bool {
        if serial < self.latest_serial {
            return false;
        }
        self.latest_serial = serial;
        if self.current.is_some_and(|item| item.id == id) {
            return false;
        }
        self.current = Some(CurrentItem { id, kind });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapid_updates_cannot_restore_an_older_selection() {
        let mut state = LiveState::default();
        assert!(state.apply_update(8, 80, ContentKind::Text));
        assert!(!state.apply_update(6, 60, ContentKind::Text));
        assert_eq!(state.current.unwrap().id, 80);
        assert!(state.apply_update(9, 90, ContentKind::Text));
    }

    #[test]
    fn newer_prepare_rejects_an_older_delayed_update() {
        let mut state = LiveState::default();
        assert!(state.apply_update(1, 10, ContentKind::Text));
        assert_eq!(state.prepare_switch(8, 80), SwitchDisposition::Ready);
        assert_eq!(state.prepare_switch(9, 90), SwitchDisposition::Ready);
        assert!(!state.apply_update(8, 80, ContentKind::Text));
        assert!(state.apply_update(9, 90, ContentKind::Image));
        assert_eq!(state.current.unwrap().id, 90);
    }

    #[test]
    fn callback_for_current_item_cannot_replace_unsaved_text() {
        let mut state = LiveState::default();
        assert!(state.apply_update(0, 44, ContentKind::Text));
        assert_eq!(state.prepare_switch(12, 44), SwitchDisposition::SameItem);
        assert!(!state.apply_update(12, 44, ContentKind::Text));
    }

    #[test]
    fn accepted_update_records_the_new_content_kind() {
        let mut state = LiveState::default();
        assert!(state.apply_update(0, 7, ContentKind::Text));
        assert!(state.apply_update(1, 8, ContentKind::Image));
        assert_eq!(
            state.current,
            Some(CurrentItem {
                id: 8,
                kind: ContentKind::Image,
            })
        );

        assert!(state.apply_update(2, 9, ContentKind::Network));
        assert_eq!(
            state.current,
            Some(CurrentItem {
                id: 9,
                kind: ContentKind::Network,
            })
        );
    }

    #[test]
    fn stale_prepare_does_not_change_the_current_item() {
        let mut state = LiveState::default();
        assert!(state.apply_update(10, 10, ContentKind::Text));
        assert_eq!(state.prepare_switch(9, 9), SwitchDisposition::Rejected);
        assert_eq!(state.current.unwrap().id, 10);
    }
}
