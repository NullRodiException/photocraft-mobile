//! Android's memory warnings (`MainActivity.onTrimMemory`, through `apps/android/src/lifecycle.rs`).
//!
//! The levels that call for something ([`trim_for`]):
//! - **The app is in the background** and among the first the system ends to free memory
//!   (`TRIM_MEMORY_BACKGROUND` and up), or is in front with memory running low
//!   (`TRIM_MEMORY_RUNNING_LOW`, Android 13 and earlier): drop the caches PhotoCraft rebuilds when
//!   it needs them ([`purge_caches`]). No frame runs in the background, so this happens in the
//!   call itself.
//! - **The app is in front and memory is critically low** (`TRIM_MEMORY_RUNNING_CRITICAL`,
//!   Android 13 and earlier): the caches go too, and the next frame autosaves the documents with
//!   unsaved changes for crash recovery ([`crate::PlatformEvent::LowMemory`]), since the system
//!   may end the app next.
//!
//! `TRIM_MEMORY_UI_HIDDEN` needs nothing: leaving the screen already autosaved
//! ([`crate::PlatformEvent::Background`]), and the caches keep the image quick to redraw when the
//! user comes back. The undo history is never dropped (that takes the user's say), nor
//! PhotoCraft's clipboard (a paste needs it, and only a frame can reach it).

/// `ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW`.
pub const RUNNING_LOW: i32 = 10;
/// `ComponentCallbacks2.TRIM_MEMORY_RUNNING_CRITICAL`.
pub const RUNNING_CRITICAL: i32 = 15;
/// `ComponentCallbacks2.TRIM_MEMORY_BACKGROUND`; `TRIM_MEMORY_MODERATE` (60) and
/// `TRIM_MEMORY_COMPLETE` (80) are higher still.
pub const BACKGROUND: i32 = 40;

/// What a memory warning calls for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trim {
    Nothing,
    /// Drop the caches.
    Caches,
    /// Drop the caches, and autosave in the next frame.
    CachesAndAutosave,
}

/// What the memory warning at `level` calls for.
pub fn trim_for(level: i32) -> Trim {
    match level {
        RUNNING_CRITICAL => Trim::CachesAndAutosave,
        RUNNING_LOW => Trim::Caches,
        l if l >= BACKGROUND => Trim::Caches,
        _ => Trim::Nothing,
    }
}

/// Drops every cache PhotoCraft rebuilds when it needs it (layer effects, masks, shape strokes,
/// smart filter stages), as Edit › Purge › All does but keeping the history and the clipboard.
/// Returns the bytes released. Safe on any thread.
pub fn purge_caches() -> usize {
    [
        photocraft_compose::purge_effect_cache(),
        photocraft_compose::masks::purge_cache(),
        photocraft_compose::shape_split::purge_cache(),
        photocraft_engine::smart_cmds::purge_stack_cache(),
    ]
    .into_iter()
    .fold(0, usize::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pressing_levels_drop_anything() {
        assert_eq!(trim_for(5), Trim::Nothing, "RUNNING_MODERATE");
        assert_eq!(trim_for(RUNNING_LOW), Trim::Caches);
        assert_eq!(trim_for(RUNNING_CRITICAL), Trim::CachesAndAutosave);
        assert_eq!(trim_for(20), Trim::Nothing, "UI_HIDDEN: leaving the screen already autosaved");
        for level in [BACKGROUND, 60, 80] {
            assert_eq!(trim_for(level), Trim::Caches, "level {level}");
        }
        assert_eq!(trim_for(-1), Trim::Nothing);
    }

    #[test]
    fn purging_releases_the_cached_masks() {
        use photocraft_compose::masks;
        use photocraft_doc::{ColorMode, Layer, LayerMask, PixelFormat, Rect, SampleType, Surface};
        // A feathered layer mask, as PhotoCraft's own purge test builds it: rendering it caches it.
        let mut input = Surface::with_default(PixelFormat { mode: ColorMode::Grayscale, sample: SampleType::U8, alpha: false }, &[1.0]);
        input.fill_rect(Rect::new(8, 8, 24, 24), &[0.0]);
        let mut layer = Layer::raster("mask", PixelFormat::RGBA8);
        layer.mask = Some(LayerMask { surface: input, feather: 2.0, ..LayerMask::reveal_all() });
        let before = masks::combined_mask(&layer, Rect::new(0, 0, 32, 32)).unwrap();
        let cached = masks::cache_bytes();
        assert!(cached > 0);
        assert!(purge_caches() >= cached, "the cached mask counts in what was released");
        assert_eq!(masks::cache_bytes(), 0);
        assert_eq!(masks::combined_mask(&layer, Rect::new(0, 0, 32, 32)).unwrap(), before, "it renders the same afterwards");
    }
}
