#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPane {
    Playlist,
    TrackInfo,
    Controls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavoritesHistoryTab {
    Favorites,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalState {
    None,
    DeviceSelect { selected_index: usize },
    ErrorAlert { message: String },
    Help { scroll_offset: usize },
    Equalizer { selected_band: usize },
    FavoritesHistory { tab: FavoritesHistoryTab, selected_index: usize },
    PlaylistManager { selected_index: usize },
    LyricsSearch {
        query: String,
        selected_index: usize,
        is_searching: bool,
        input_mode: bool,
    },
}

#[derive(Debug, Clone)]
pub struct UiHfsm {
    pub active_pane: UiPane,
    pub modal: ModalState,
}

impl UiHfsm {
    pub fn new() -> Self {
        Self {
            active_pane: UiPane::Playlist,
            modal: ModalState::None,
        }
    }

    pub fn next_pane(&mut self) {
        if self.modal != ModalState::None {
            return;
        }
        self.active_pane = match self.active_pane {
            UiPane::Playlist => UiPane::TrackInfo,
            UiPane::TrackInfo => UiPane::Controls,
            UiPane::Controls => UiPane::Playlist,
        };
    }

    pub fn prev_pane(&mut self) {
        if self.modal != ModalState::None {
            return;
        }
        self.active_pane = match self.active_pane {
            UiPane::Playlist => UiPane::Controls,
            UiPane::TrackInfo => UiPane::Playlist,
            UiPane::Controls => UiPane::TrackInfo,
        };
    }

    pub fn open_modal(&mut self, modal: ModalState) {
        self.modal = modal;
    }

    pub fn close_modal(&mut self) {
        self.modal = ModalState::None;
    }

    pub fn is_modal_open(&self) -> bool {
        self.modal != ModalState::None
    }
}

impl Default for UiHfsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_hfsm_pane_cycling_and_modal_isolation() {
        let mut ui = UiHfsm::new();
        assert_eq!(ui.active_pane, UiPane::Playlist);
        assert_eq!(ui.modal, ModalState::None);

        ui.next_pane();
        assert_eq!(ui.active_pane, UiPane::TrackInfo);

        ui.next_pane();
        assert_eq!(ui.active_pane, UiPane::Controls);

        ui.next_pane();
        assert_eq!(ui.active_pane, UiPane::Playlist);

        ui.prev_pane();
        assert_eq!(ui.active_pane, UiPane::Controls);

        // モーダルを開いている時はペイン移動が無効化される
        ui.open_modal(ModalState::Help { scroll_offset: 0 });
        assert!(ui.is_modal_open());

        ui.next_pane();
        assert_eq!(ui.active_pane, UiPane::Controls); // 変化しない

        ui.close_modal();
        assert!(!ui.is_modal_open());
        assert_eq!(ui.modal, ModalState::None);
    }
}

