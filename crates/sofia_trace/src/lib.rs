//! Separate GPUI Kit desktop client for Sofia runs and tool timelines.
mod table;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::Button,
    tab::{Tab, TabBar},
    table::{DataTable, TableEvent, TableState},
    text::{TextView, TextViewState},
};
use gpui_kit::*;
use sofia_trace_store::{Run, Store};
use std::time::Duration;
use table::{RunRows, StepRows};

pub struct TraceView {
    store: Option<Store>,
    version: i64,
    runs: Vec<Run>,
    selected: Option<String>,
    follow_latest: bool,
    tab: usize,
    run_table: Entity<TableState<RunRows>>,
    step_table: Entity<TableState<StepRows>>,
    input: Entity<TextViewState>,
    output: Entity<TextViewState>,
    failure: Entity<TextViewState>,
    error: String,
}

impl TraceView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let run_table = cx.new(|cx| {
            TableState::new(RunRows(vec![]), window, cx)
                .row_selectable(true)
                .col_resizable(false)
                .col_movable(false)
                .sortable(false)
        });
        let step_table = cx.new(|cx| {
            TableState::new(
                StepRows {
                    rows: vec![],
                    started_ms: 0,
                },
                window,
                cx,
            )
            .row_selectable(false)
            .col_resizable(false)
            .col_movable(false)
            .sortable(false)
        });
        cx.subscribe(&run_table, |view, _, event: &TableEvent, cx| {
            if let TableEvent::SelectRow(index) = event {
                view.select(*index, cx);
            }
        })
        .detach();
        let input = cx.new(|cx| TextViewState::markdown("", cx).selectable(true));
        let output = cx.new(|cx| TextViewState::markdown("", cx).selectable(true));
        let failure = cx.new(|cx| TextViewState::markdown("", cx).selectable(true));
        let result = Store::default_path().and_then(Store::open);
        let (store, error) = match result {
            Ok(store) => (Some(store), String::new()),
            Err(error) => (None, error),
        };
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
        let mut view = Self {
            store,
            version: -1,
            runs: vec![],
            selected: None,
            follow_latest: true,
            tab: 0,
            run_table,
            step_table,
            input,
            output,
            failure,
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
                self.runs = runs;
                self.run_table.update(cx, |table, cx| {
                    table.delegate_mut().0 = self.runs.clone();
                    table.refresh(cx);
                });
                self.update_detail(cx);
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

    fn update_detail(&mut self, cx: &mut Context<Self>) {
        let run = self
            .runs
            .iter()
            .find(|run| self.selected.as_ref() == Some(&run.id));
        let (steps, started, input, output, failure) = if let Some(run) = run {
            (
                self.store
                    .as_ref()
                    .and_then(|store| store.steps(&run.id).ok())
                    .unwrap_or_default(),
                run.started_ms,
                run.input.clone(),
                run.output.clone(),
                run.error.clone(),
            )
        } else {
            (vec![], 0, String::new(), String::new(), String::new())
        };
        self.step_table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.rows = steps;
            delegate.started_ms = started;
            table.refresh(cx);
        });
        self.input
            .update(cx, |state, cx| state.set_text(&input, cx));
        self.output
            .update(cx, |state, cx| state.set_text(&output, cx));
        self.failure
            .update(cx, |state, cx| state.set_text(&failure, cx));
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(id) = self.runs.get(index).map(|run| run.id.clone()) else {
            return;
        };
        if self.selected.as_ref() == Some(&id) {
            return;
        }
        self.selected = Some(id);
        self.follow_latest = false;
        self.update_detail(cx);
        cx.notify();
    }

    fn latest(&mut self, cx: &mut Context<Self>) {
        self.follow_latest = true;
        self.selected = self.runs.first().map(|run| run.id.clone());
        self.update_detail(cx);
        if !self.runs.is_empty() {
            self.run_table
                .update(cx, |table, cx| table.set_selected_row(0, cx));
        }
        cx.notify();
    }
}

impl Render for TraceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = TabBar::new("trace-tabs")
            .underline()
            .selected_index(self.tab)
            .child(Tab::new().label("Timeline"))
            .child(Tab::new().label("Input"))
            .child(Tab::new().label("Sofia"))
            .child(Tab::new().label("Errors"))
            .on_click(cx.listener(|view, index, _, cx| {
                view.tab = *index;
                cx.notify();
            }));
        let detail = match self.tab {
            1 => TextView::new(&self.input)
                .scrollable(true)
                .size_full()
                .into_any_element(),
            2 => TextView::new(&self.output)
                .scrollable(true)
                .size_full()
                .into_any_element(),
            3 => TextView::new(&self.failure)
                .scrollable(true)
                .size_full()
                .into_any_element(),
            _ => DataTable::new(&self.step_table).into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child("Sofia Trace")
                    .child(if self.error.is_empty() {
                        format!("{} runs", self.runs.len())
                    } else {
                        self.error.clone()
                    })
                    .child(
                        Button::new("latest")
                            .icon(IconName::RefreshCw)
                            .on_click(cx.listener(|view, _, _, cx| view.latest(cx))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .w(relative(0.35))
                            .min_w_0()
                            .child(DataTable::new(&self.run_table)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(tabs)
                            .child(div().flex_1().min_h_0().child(detail)),
                    ),
            )
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
