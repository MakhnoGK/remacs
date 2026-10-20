use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    AnyElement, Context, Div, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Size,
    Window, div, prelude::*, px, rgb, rgba,
};
use remacs_core::{
    Action, Code, Editor, Outline, Pane, PaneBody, Popup, Preview, StatusItem, StatusSegment,
    WindowId, describe_sequence,
};
use remacs_echo::VIEW_MESSAGES;
use remacs_frame::TEXT_CONFIGURED;
use remacs_theme::{color, font, glass};

use super::{
    CLOSE_KEY, COPY_MESSAGE_KEY, EditorView, Metrics, OpenMessage, PromptSurface, ROW_MARGIN,
    TEXT_TOP, TITLE_BAR_HEIGHT,
};
use crate::bottom_row::{
    CardKeys, FittedHints, Hints, PrefixKeys, echo_room, fit_hints, render_hints,
    render_hints_list, render_message_card, render_message_echo, render_prefix_echo,
    render_prefix_keys, render_row,
};
use crate::highlights::{Highlights, face_style};
use crate::minibuffer_view::{is_problem_list, render_minibuffer, render_scrim};
use crate::outline_view::{
    COMMIT_HEIGHT, OutlineScrolled, fits_history, fitting_commits, fitting_rows, render_outline,
    render_toolbar, row_height,
};
use crate::pane_view::{PaneChrome, render_pane, screen_bounds};
use crate::popup_view::{Placement, PopupView, render_popup};
use crate::preview_view::render_preview;
use crate::transition::{Retained, Side};
use crate::widgets::{Scrolled, icon, modified_dot, over, pill, pill_separator, tone_color};

/// Per mode; one popup shows a handful.
const MAX_HIGHLIGHTED_SNIPPETS: usize = 64;

impl EditorView {
    /// Popups next to the text they belong to, e.g. completions under the word or the
    /// documentation of a symbol; placed in the whole `frame_size`, as one may reach over
    /// other windows.
    pub(super) fn render_popups(
        &mut self,
        metrics: Metrics,
        gutter_width: Pixels,
        frame_size: Size<Pixels>,
    ) -> Vec<AnyElement> {
        let origin = self.window_state.frame.origin;
        let window_bottom = self.window_state.clip.bottom();
        let line_height = px(self.text_font().line_height);
        let transition = self.transitions.popup();
        let slots: Vec<Option<Popup>> = self.editor.popup_slots().collect();
        self.documentation_scroll.update(slots.iter().flatten());
        let popups = &mut self.surfaces.popups;
        popups.resize_with(slots.len(), Retained::default);
        let now = Instant::now();
        for (retained, popup) in popups.iter_mut().zip(slots) {
            retained.update(popup, now, transition.duration);
        }
        let code_faces = |code: &Code| self.faces_of(code);
        let view = PopupView {
            code_faces: &code_faces,
            scroll: &self.documentation_scroll.handle,
            mono_width: metrics.mono_width,
            small_mono_width: metrics.small_mono_width,
            ui_char_width: metrics.ui_char_width,
            max_width: px(self.documentation.max_width),
            max_height: px(self.documentation.max_height),
            notes_max_width: px(self.problems.max_width),
            notes_max_height: px(self.problems.max_height),
            completion_swatches: self.swatches.completion,
        };
        self.surfaces
            .popups
            .iter()
            .filter_map(|retained| retained.shown(transition))
            .filter_map(|shown| {
                let popup = shown.content;
                let line = self.editor.window_buffer().text().line_of(popup.anchor);
                let row = line as f32 - self.window_state.vertical.shown();
                if row < 0.0 {
                    return None;
                }
                let line_top = origin.y + px(TEXT_TOP) + line_height * row;
                if line_top >= window_bottom {
                    return None;
                }
                let (display_line, visible) = self.display_line(line);
                let offset = popup.anchor.clamp(visible.start, visible.end) - visible.start;
                let byte = display_line.display_range(offset..offset).start;
                let column = (display_line.first_column()
                    + display_line.text[..byte].chars().count())
                    as f32
                    - self.window_state.sideways.shown();
                let anchor_left = origin.x
                    + self.window_state.text_left
                    + px(ROW_MARGIN)
                    + gutter_width
                    + metrics.char_width * column.max(0.0);
                let placement = Placement {
                    anchor_left,
                    line_top,
                    line_bottom: line_top + line_height,
                    area_top: px(TITLE_BAR_HEIGHT),
                    window_width: frame_size.width,
                    window_height: frame_size.height,
                };
                Some(shown.look.apply(render_popup(popup, placement, &view)))
            })
            .collect()
    }

    /// The pane under the text as it comes, is, or goes; `current` is the one shown now.
    pub(super) fn render_pane(
        &mut self,
        current: Option<(Pane, PaneChrome)>,
        screen_owners: &HashMap<String, WindowId>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let transition = self.transitions.pane();
        self.surfaces
            .pane
            .update(current, Instant::now(), transition.duration);
        let (pane, _) = self.surfaces.pane.shown(transition)?.content;
        // A screen is sized to one place a frame: a window's, if one shows it too.
        let screen = match &pane.body {
            PaneBody::Screen(name) if !screen_owners.contains_key(name) => {
                Some((name.clone(), pane.height))
            }
            _ => None,
        };
        let screen = screen.and_then(|(name, height)| {
            let bounds = screen_bounds(window.viewport_size(), height);
            let under = over(glass::pane(), glass::window_tint());
            self.render_screen_in(&name, bounds, Point::default(), under, window, cx)
        });
        let shown = self.surfaces.pane.shown(transition)?;
        let (pane, chrome) = shown.content;
        let element = render_pane(pane, chrome, screen, cx)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        Some(shown.look.apply(element))
    }

    /// The prompt and the scrim behind it as they come, are, or go.
    pub(super) fn render_prompt(&mut self, cx: &mut Context<Self>) -> Option<PromptSurface> {
        let transition = self.transitions.prompt();
        let closed = self.editor.take_closed_minibuffer();
        let open = self.editor.minibuffer().is_some();
        let retained = &mut self.surfaces.prompt;
        retained.update_owned(open, closed, Instant::now(), transition.duration);
        let minibuffer = self.editor.minibuffer().or(retained.leaving())?;
        let problem_list = is_problem_list(minibuffer);
        let side = if problem_list {
            Side::Below
        } else {
            Side::Above
        };
        let look = retained.look(transition.coming_from(side))?;
        let scrim = render_scrim(minibuffer.dialog().is_some());
        Some(PromptSurface {
            surface: look.apply(render_minibuffer(
                minibuffer,
                &self.input_scroll,
                self.prompts,
                cx,
            )),
            scrim: look.in_place().apply(scrim),
            problem_list,
        })
    }

    /// Parses `code` the first time it is asked for; documentation's code is parsed as it
    /// was wrapped to fit, which only the drawing knows.
    fn faces_of(&self, code: &Code) -> Highlights {
        let Some(mode) = code.mode else {
            return Highlights::default();
        };
        let mut code_faces = self.code_faces.borrow_mut();
        let known = code_faces.entry(mode).or_default();
        if let Some(faces) = known.get(&code.text) {
            return faces.clone();
        }
        if known.len() >= MAX_HIGHLIGHTED_SNIPPETS {
            known.clear();
        }
        let faces: Highlights = self
            .editor
            .highlight_code(mode, &code.text)
            .into_iter()
            .map(|span| (span.range, face_style(span.face)))
            .collect();
        known.insert(code.text.clone(), faces.clone());
        faces
    }

    pub(super) fn render_preview(&mut self, preview: Preview) -> Div {
        render_preview(preview, &|code| self.faces_of(code))
    }

    /// The window buffer's name in the middle of the title bar, unless chips name it; with
    /// other windows, its own name, bright only in the selected one.
    pub(super) fn render_header(&self, titled: bool, split: bool) -> Div {
        let buffer = self.editor.window_buffer();
        let header = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TITLE_BAR_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .font_family(font::ui())
            .text_size(px(font::ui_size()));
        if !titled {
            return header;
        }
        if split {
            let selected = self.drawing_selected();
            return header.child(
                div()
                    .when(selected, |name| name.font_weight(gpui::FontWeight::MEDIUM))
                    .text_color(rgb(if selected {
                        color::fg_main()
                    } else {
                        color::fg_dim()
                    }))
                    .child(buffer.name().to_owned()),
            );
        }
        header
            .child(
                div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(buffer.name().to_owned()),
            )
            .child(div().text_color(rgb(color::fg_dim())).child("— Remacs"))
    }

    /// The window buffer's outline in the rows that fit `height`, the viewport set to them.
    pub(super) fn render_outline(
        &mut self,
        outline_lines: usize,
        height: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Hints, Option<AnyElement>)> {
        let history_shown = self.advance_history(window, cx);
        let mut outline = self.editor.outline(
            self.editor.viewport.first_line..self.editor.viewport.first_line + outline_lines,
        )?;
        self.window_state.history_scroll = outline
            .history
            .as_ref()
            .map(|history| history.scroll.clone());
        let fit = |outline: &Outline| {
            let has_banner = outline.banner.is_some();
            fitting_rows(&outline.rows, outline.facts.len(), has_banner, height)
        };
        let mut fitting = fit(&outline);
        let point_line = self.editor.window_buffer().point_line();
        let shown_from = self.editor.viewport.first_line;
        self.editor.viewport.set_height(fitting);
        self.editor.viewport.margins = (1, 1);
        self.editor.show_point();
        if self.editor.viewport.first_line != shown_from {
            let first_line = self.editor.viewport.first_line;
            outline = self
                .editor
                .outline(first_line..first_line + outline_lines)?;
            fitting = fit(&outline);
        }
        self.editor.viewport.set_height(fitting);
        let top = self.advance_scroll(fitting, window);
        let first_line = top as usize;
        if first_line != self.editor.viewport.first_line {
            outline = self
                .editor
                .outline(first_line..first_line + outline_lines)?;
            fitting = fit(&outline);
        }
        let mut scrolled = OutlineScrolled {
            rows: Scrolled {
                above: top > 0.0,
                below: outline.rows.len() > fitting,
            },
            rows_shift: row_height(&outline.rows, 0) * top.fract(),
            history: Scrolled::default(),
            history_shift: 0.0,
        };
        // One more for the row partly scrolled in at the bottom.
        outline.rows.truncate(fitting + 1);
        if !fits_history(self.window_state.frame.size.width) {
            outline.history = None;
            outline.conflict = None;
        }
        let has_banner = outline.banner.is_some();
        if let Some(history) = &mut outline.history {
            let fitting = fitting_commits(height, has_banner);
            self.window_state.history_scrolled = Scrolled {
                above: history.scrolled,
                below: history.commits.len() > fitting,
            };
            // With the oldest commit in view, part of one scrolled past glides back.
            if !self.window_state.history_scrolled.below {
                self.window_state.history_target = self.window_state.history_target.min(0.0);
            }
            scrolled.history = Scrolled {
                above: history.scrolled || history_shown > 0.0,
                ..self.window_state.history_scrolled
            };
            scrolled.history_shift = COMMIT_HEIGHT * history_shown;
            history.commits.truncate(fitting + 1);
            // The selected commit is scrolled into sight for the next frame.
            if let Some(selected) = history
                .selected
                .filter(|&selected| fitting > 0 && selected >= fitting)
            {
                let (scroll, rows) = (history.scroll.clone(), (selected + 1 - fitting) as isize);
                let action: Action = Rc::new(move |editor| scroll(editor, rows));
                self.run_action(&action, cx);
            }
        }
        let hints = outline.hints;
        let toolbar = std::mem::take(&mut outline.toolbar);
        let toolbar = (!toolbar.is_empty()).then(|| render_toolbar(toolbar, cx).into_any_element());
        let faces = |code: &Code| self.faces_of(code);
        let body = render_outline(outline, first_line, point_line, scrolled, &faces, cx)
            .into_any_element();
        Some((body, hints, toolbar))
    }

    /// A plugin's group of the status pill, e.g. problem counts; a click runs its command.
    /// One without a command is a plain label, like the mode.
    fn render_segment(segment: StatusSegment, cx: &mut Context<Self>) -> impl IntoElement {
        let text_color = match (segment.active, segment.command, segment.code) {
            (true, ..) => color::fg_main(),
            (false, Some(_), _) | (false, None, true) => color::fg_kbd(),
            (false, None, false) => color::fg_faint(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(24.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .when(segment.active, |button| button.bg(rgba(glass::selection())))
            .font_family(if segment.code {
                font::mono()
            } else {
                font::ui()
            })
            .text_color(rgb(text_color))
            .when_some(segment.command, |button, command| {
                button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        view.run_command(command, cx);
                    }),
                )
            })
            .children(segment.items.into_iter().map(|item| {
                let item_color = tone_color(item.tone, text_color);
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .text_color(rgb(item_color))
                    .children(item.icon.map(|item_icon| icon(item_icon, item_color)))
                    .child(item.text)
            }))
    }

    /// The row along the bottom, and what opens from it over the view: the message whole,
    /// the keys that do not fit, the keys after a prefix.
    pub(super) fn render_bottom_row(
        &mut self,
        hints: Hints,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Vec<AnyElement>) {
        let settings = self.bottom;
        let fitted = fit_hints(hints, settings.hints.show, settings.hints.side_room, window);
        let key_for = |editor: &Editor, command: &str| {
            editor
                .keymap()
                .sequences_for(command)
                .min_by_key(|keys| keys.len())
                .map(describe_sequence)
        };
        let whole_key = key_for(&self.editor, VIEW_MESSAGES);
        self.track_message(cx);
        let transition = self.transitions.popup().coming_from(Side::Below);
        let message = self.editor.message().cloned();
        let echo_pill = &mut self.surfaces.echo;
        echo_pill.update(message, Instant::now(), transition.duration);
        let pending = self.editor.pending_keys();
        // Keys typed show at once, in place of the message.
        let echo = if !pending.is_empty() {
            Some(render_prefix_echo(pending).into_any_element())
        } else {
            echo_pill.shown(transition).map(|shown| {
                let pill = render_message_echo(
                    shown.content,
                    echo_room(&fitted, window),
                    self.message_open,
                    whole_key.as_deref(),
                    window,
                    cx,
                );
                shown.look.apply_in_flow(pill)
            })
        };
        let listed = self.hints_open || self.editor.key_hints_listed();
        let hints_pill = render_hints(&fitted, listed, cx);
        let row = render_row(echo, hints_pill, self.render_status(cx).into_any_element());

        let message = self
            .editor
            .message()
            .filter(|_| self.message_open)
            .map(|message| OpenMessage {
                message: message.clone(),
                all_messages: whole_key,
            });
        let keys = (listed && !fitted.listed().is_empty()).then(|| {
            let title = match self.editor.shown_pane() {
                Some(pane) if self.pane_keys_listed() => pane,
                _ => self.editor.window_buffer().mode(),
            };
            (fitted, title)
        });
        let prefix_keys = (self.prefix_keys_due() && !pending.is_empty())
            .then(|| PrefixKeys::new(pending, self.editor.prefix_continuations()));
        (
            row.into_any_element(),
            self.render_cards(message, keys, prefix_keys, window),
        )
    }

    /// Whether the keys listed are a shown pane's: always under text, under rows or a
    /// screen only while it has them, since theirs are what those are for.
    pub(super) fn pane_keys_listed(&self) -> bool {
        let state = &self.window_state;
        self.editor.pane_focused() || (!state.showing_screen && !state.showing_rows)
    }

    /// Over the bottom row, rising from it, as they come, are, or go.
    fn render_cards(
        &mut self,
        message: Option<OpenMessage>,
        keys: Option<(FittedHints, &'static str)>,
        prefix_keys: Option<PrefixKeys>,
        window: &Window,
    ) -> Vec<AnyElement> {
        let transition = self.transitions.popup().coming_from(Side::Below);
        let (now, duration) = (Instant::now(), transition.duration);
        let surfaces = &mut self.surfaces;
        surfaces.message.update(message, now, duration);
        surfaces.keys.update(keys, now, duration);
        surfaces.prefix_keys.update(prefix_keys, now, duration);
        let settings = self.bottom;
        let message = surfaces.message.shown(transition).map(|shown| {
            let card = shown.content;
            let keys = CardKeys {
                all_messages: card.all_messages.as_deref(),
                copy: COPY_MESSAGE_KEY,
                close: CLOSE_KEY,
            };
            let lines = settings.echo.lines;
            shown
                .look
                .apply(render_message_card(&card.message, lines, keys, window))
        });
        let keys = surfaces.keys.shown(transition).and_then(|shown| {
            let (fitted, title) = shown.content;
            render_hints_list(fitted, title).map(|list| shown.look.apply(list))
        });
        let prefix_keys = surfaces.prefix_keys.shown(transition).map(|shown| {
            let columns = settings.prefix_keys.columns;
            let card = render_prefix_keys(shown.content, columns, CLOSE_KEY, window);
            shown.look.apply(card)
        });
        message.into_iter().chain(keys).chain(prefix_keys).collect()
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buffer = self.editor.window_buffer();
        let position = format!("{}:{}", buffer.point_line() + 1, buffer.point_column() + 1);
        // Only text zooms; a click gives it the configured size back.
        let zoom = self
            .text_zoom
            .percent(font::code().size)
            .filter(|_| !self.window_state.showing_rows && !self.window_state.showing_screen)
            .map(|percent| StatusSegment {
                items: vec![StatusItem::new(None, format!("{percent}%"))],
                command: Some(TEXT_CONFIGURED),
                active: false,
                code: true,
            });
        let mut segments = self
            .editor
            .status_segments()
            .into_iter()
            .chain(zoom)
            .flat_map(|segment| {
                [
                    Self::render_segment(segment, cx).into_any_element(),
                    pill_separator().into_any_element(),
                ]
            })
            .collect::<Vec<_>>();
        if self.window_state.showing_screen {
            // A screen shows neither position nor mode after the segments.
            segments.pop();
        }

        pill()
            .flex_none()
            .children(segments)
            // The selected row is the position; a screen has none.
            .when(
                !self.window_state.showing_rows && !self.window_state.showing_screen,
                |status| {
                    status
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .px(px(8.0))
                                .font_family(font::mono())
                                .when(buffer.is_modified(), |position| {
                                    position.child(modified_dot())
                                })
                                .child(position),
                        )
                        .child(pill_separator())
                },
            )
            .when(!self.window_state.showing_screen, |status| {
                status.child(
                    div()
                        .pl(px(8.0))
                        .pr(px(10.0))
                        .font_family(font::ui())
                        .child(mode_label(
                            buffer.mode(),
                            self.editor.language_at(buffer, buffer.point()),
                        )),
                )
            })
    }
}

/// The buffer's mode, and the language at point when it is another, like `HTML › CSS`.
fn mode_label(mode: &'static str, language: &'static str) -> SharedString {
    if language == mode {
        mode.into()
    } else {
        format!("{mode} › {language}").into()
    }
}
