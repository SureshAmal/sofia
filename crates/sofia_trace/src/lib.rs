//! Separate desktop client for live activity and persisted trace reports.
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::Button,
    scroll::ScrollableElement,
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sofia_trace_store::{Run, Step, Store};
use std::time::Duration;

pub struct TraceView {
    store: Option<Store>,
    version: i64,
    runs: Vec<Run>,
    selected: Option<String>,
    follow_latest: bool,
    steps: Vec<Step>,
    error: String,
}

impl TraceView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if this.update(cx, |view, cx| view.refresh(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let result = Store::default_path().and_then(Store::open);
        let (store, error) = match result {
            Ok(store) => (Some(store), String::new()),
            Err(error) => (None, error),
        };
        let mut view = Self {
            store,
            version: -1,
            runs: vec![],
            selected: None,
            follow_latest: true,
            steps: vec![],
            error,
        };
        view.refresh(cx);
        view
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(store) = &self.store else { return };
        let Ok(version) = store.data_version() else {
            return;
        };
        if version == self.version {
            return;
        }
        match store.runs() {
            Ok(runs) => {
                if self.follow_latest
                    || self
                        .selected
                        .as_ref()
                        .is_none_or(|id| !runs.iter().any(|run| &run.id == id))
                {
                    self.selected = runs.first().map(|run| run.id.clone());
                }
                self.steps = self
                    .selected
                    .as_ref()
                    .and_then(|id| store.steps(id).ok())
                    .unwrap_or_default();
                self.runs = runs;
                self.version = version;
                self.error.clear();
                cx.notify();
            }
            Err(error) => {
                self.error = error;
                cx.notify();
            }
        }
    }

    fn select(&mut self, id: String, cx: &mut Context<Self>) {
        self.steps = self
            .store
            .as_ref()
            .and_then(|store| store.steps(&id).ok())
            .unwrap_or_default();
        self.selected = Some(id);
        self.follow_latest = false;
        cx.notify();
    }

    fn latest(&mut self, cx: &mut Context<Self>) {
        self.follow_latest = true;
        self.selected = self.runs.first().map(|run| run.id.clone());
        self.steps = self
            .selected
            .as_ref()
            .and_then(|id| self.store.as_ref()?.steps(id).ok())
            .unwrap_or_default();
        cx.notify();
    }

    fn runs_table(&self, cx: &mut Context<Self>) -> Table {
        let mut body = TableBody::new();
        for run in &self.runs {
            let id = run.id.clone();
            let selected = self.selected.as_ref() == Some(&id);
            let title = if run.input.is_empty() {
                "(system)".to_string()
            } else {
                summary(&run.input, 54)
            };
            body = body.child(
                TableRow::new()
                    .child(
                        TableCell::new().child(
                            Button::new(SharedString::from(id.clone()))
                                .label(if selected {
                                    format!("● {title}")
                                } else {
                                    title
                                })
                                .on_click(
                                    cx.listener(move |view, _, _, cx| view.select(id.clone(), cx)),
                                ),
                        ),
                    )
                    .child(TableCell::new().child(format!(
                        "{} · {} tools · {}",
                        run.status,
                        run.tool_count,
                        duration(run.duration_ms)
                    ))),
            );
        }
        Table::new()
            .small()
            .accessibility_label("Recent Sofia runs")
            .child(
                TableHeader::new().child(
                    TableRow::new()
                        .child(TableHead::new().child("Run"))
                        .child(TableHead::new().child("Result")),
                ),
            )
            .child(body)
    }

    fn steps_table(&self, started_ms: i64) -> Table {
        let mut body = TableBody::new();
        for step in &self.steps {
            body = body.child(
                TableRow::new()
                    .child(TableCell::new().child(step.kind.clone()))
                    .child(TableCell::new().child(step.name.clone()))
                    .child(TableCell::new().child(format!(
                        "+{} ms",
                        step.started_ms.saturating_sub(started_ms)
                    )))
                    .child(
                        TableCell::new().child(
                            step.duration_ms
                                .map(|value| format!("{value} ms"))
                                .unwrap_or_default(),
                        ),
                    )
                    .child(TableCell::new().child(summary(&step.detail, 96))),
            );
        }
        Table::new()
            .small()
            .accessibility_label("Selected run timeline")
            .child(
                TableHeader::new().child(
                    TableRow::new()
                        .child(TableHead::new().child("Event"))
                        .child(TableHead::new().child("Name"))
                        .child(TableHead::new().child("At"))
                        .child(TableHead::new().child("Duration"))
                        .child(TableHead::new().child("Detail")),
                ),
            )
            .child(body)
    }
}

fn summary(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = line.chars();
    let start: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{start}…")
    } else {
        start
    }
}
fn duration(ms: Option<i64>) -> String {
    ms.map(|ms| format!("{:.1}s", ms as f64 / 1000.))
        .unwrap_or_else(|| "live".into())
}

impl Render for TraceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self
            .runs
            .iter()
            .find(|run| self.selected.as_ref() == Some(&run.id));
        let started_ms = selected.map(|run| run.started_ms).unwrap_or_default();
        let (input, output, error) = selected
            .map(|run| (run.input.clone(), run.output.clone(), run.error.clone()))
            .unwrap_or_default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Sofia Trace"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.error.is_empty() {
                        format!("{} recent runs · updates live", self.runs.len())
                    } else {
                        self.error.clone()
                    }),
            )
            .child(
                Button::new("latest-run")
                    .label("Latest run")
                    .on_click(cx.listener(|view, _, _, cx| view.latest(cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap_4()
                    .child(
                        div()
                            .w(relative(0.35))
                            .min_w_0()
                            .overflow_y_scrollbar()
                            .child(self.runs_table(cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scrollbar()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().text_lg().child("Timeline"))
                            .child(self.steps_table(started_ms))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Input"),
                            )
                            .child(div().text_sm().child(input))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Sofia"),
                            )
                            .child(div().text_sm().child(output))
                            .when(!error.is_empty(), |this| {
                                this.child(div().text_color(cx.theme().danger).child(error))
                            }),
                    ),
            )
    }
}
