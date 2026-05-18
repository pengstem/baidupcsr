# TUI UI Quality Pass Design

## Direction

Use the Focused Browser layout direction.

The main surface should prioritize two things: a fast file browser and a useful preview panel. The old left context rail should stop behaving like a separate "Locations" area. Its useful information moves into a compact breadcrumb/status strip and small contextual lines only when they add value.

The `?` key opens a help overlay, but the UI should not visually shout about it. The normal screen may show no help hint, or at most a quiet `? help` in the status line if space allows.

## Interaction

- Vim keys stay supported: `j/k/h/l`, `/`, `:`, `space`, `r`, `d`, `x`, `p`, `q`.
- Arrow keys mirror Vim navigation: Up/Down select, Left goes parent, Right enters directory.
- `?` toggles a compact help overlay with current keybindings and mouse actions.
- Mouse support is enabled in the terminal session.
- Left click on a file row selects it and emits the same preview/preload refresh path as keyboard movement.
- Double click or right-arrow-equivalent activation on a directory enters it.
- Mouse wheel scrolls selection through the visible file list without breaking Vim behavior.

## Preview Coverage

Preview planning should expand beyond text, images, PDF, archive, directory, metadata, and video.

Add at least these file classes:

- Audio: metadata-only for now, with type, size, and play/open guidance.
- Office documents: `doc`, `docx`, `ppt`, `pptx`, `xls`, `xlsx`, `odt`, `ods`, `odp` as document metadata previews.
- E-books: `epub`, `mobi`, `azw3` as document metadata previews.
- Fonts: `ttf`, `otf`, `woff`, `woff2` as font metadata previews.
- Checksums/torrents/playlists: text-like preview where safely small.

This pass should not decode full PDF pages, Office files, archives, or audio streams. It should classify them correctly, show richer metadata, and keep memory/disk behavior explicit.

## Visual Design

- Remove redundant diagnostic noise from the default right panel.
- Use human-readable labels instead of raw `key=value` for ordinary metadata.
- Keep raw diagnostic lines only for video playback and errors where they are actionable.
- Preview titles and paths must wrap or use middle truncation so the important suffix remains visible.
- File rows should keep stable dimensions and a clearer selected-row style.
- The preview panel should use its available space: title, type badges, size/mtime/path, preview status, then content or metadata.
- The status bar should be shorter and calmer; avoid long repeated hints.

## Validation

Automated tests must cover:

- `?` toggles help mode/overlay without making help visible by default.
- Arrow keys map to the same actions as Vim keys.
- Mouse click/wheel events map to selection/navigation runtime events.
- Render tests prove the Focused Browser layout removes the vague left rail and shows Preview details without losing filename suffixes.
- Preview planner tests cover the new file classes.

Manual tmux validation must use the local BaiduPCS-Go config and real data:

- Open the TUI, verify the cleaned layout.
- Press `?`, verify the help overlay, then close it.
- Use arrow keys and Vim keys interchangeably.
- Use mouse click and wheel in the file list.
- Select real text, image, PDF, archive, audio/document-like files when available and verify the Preview panel is not empty or misleading.

## Out Of Scope

- Full PDF rendering.
- Office document parsing.
- Archive file listing by downloading and parsing archive contents.
- Replacing mpv playback architecture.
- Theme configuration files.
