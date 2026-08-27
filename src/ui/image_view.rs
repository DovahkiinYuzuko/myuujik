use crate::audio::decoder::CoverArt;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;

pub struct CoverArtWidget {
    cached_track_path: Option<String>,
    image_protocol: Option<StatefulProtocol>,
    picker: Option<Picker>,
}

impl CoverArtWidget {
    pub fn new() -> Self {
        // ターミナルプロトコルの自動検出（Kitty, Sixel, Halfblocks）
        let picker = Picker::from_query_stdio().ok();
        Self {
            cached_track_path: None,
            image_protocol: None,
            picker,
        }
    }

    /// カバーアートの即時リサイズ（<=300x300）およびシングルキャッシュ更新
    pub fn update_cover_art(&mut self, track_key: &str, cover: Option<&CoverArt>) {
        if self.cached_track_path.as_deref() == Some(track_key) {
            return; // 既にキャッシュ済み
        }

        self.cached_track_path = Some(track_key.to_string());
        self.image_protocol = None;

        if let Some(cover_data) = cover {
            if let Ok(dyn_img) = image::load_from_memory(&cover_data.data) {
                // RSS 35MB以下を死守するため、即座に最大300x300へダウンサンプリング
                let resized = dyn_img.thumbnail(300, 300);
                if let Some(picker) = &mut self.picker {
                    let proto = picker.new_resize_protocol(resized);
                    self.image_protocol = Some(proto);
                }
            }
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if let Some(proto) = &mut self.image_protocol {
            let image_widget = StatefulImage::new(None);
            StatefulWidget::render(image_widget, area, buf, proto);
        }
    }

    pub fn has_image(&self) -> bool {
        self.image_protocol.is_some()
    }
}

impl Default for CoverArtWidget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cover_art_widget_initialization() {
        let mut widget = CoverArtWidget::new();
        assert!(!widget.has_image());
        widget.update_cover_art("empty_track", None);
        assert!(!widget.has_image());
    }
}

