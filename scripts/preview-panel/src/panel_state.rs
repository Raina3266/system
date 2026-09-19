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
