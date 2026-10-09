use gpui_kit::component::table::{Column, TableDelegate};
use gpui_kit::*;
use sofia_trace_store::{Run, Step};

pub struct RunRows(pub Vec<Run>);

impl TableDelegate for RunRows {
    fn columns_count(&self, _: &App) -> usize {
        2
    }
    fn rows_count(&self, _: &App) -> usize {
        self.0.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        match index {
            0 => Column::new("run", "Run").width(px(320.)),
            _ => Column::new("result", "Result").width(px(190.)),
        }
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        _: &mut Context<gpui_kit::component::table::TableState<Self>>,
    ) -> impl IntoElement {
        let run = &self.0[row];
        match column {
            0 => super::summary(
                if run.input.is_empty() {
                    "(system)"
                } else {
                    &run.input
                },
                68,
            ),
            _ => format!(
                "{} · {} tools · {}",
                run.status,
                run.tool_count,
                super::duration(run.duration_ms)
            ),
        }
    }
}

pub struct StepRows {
    pub rows: Vec<Step>,
    pub started_ms: i64,
}

impl TableDelegate for StepRows {
    fn columns_count(&self, _: &App) -> usize {
        5
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        match index {
            0 => Column::new("at", "At").width(px(90.)),
            1 => Column::new("event", "Event").width(px(150.)),
            2 => Column::new("name", "Name").width(px(180.)),
            3 => Column::new("duration", "Duration").width(px(100.)),
            _ => Column::new("detail", "Detail").width(px(360.)),
        }
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        _: &mut Context<gpui_kit::component::table::TableState<Self>>,
    ) -> impl IntoElement {
        let step = &self.rows[row];
        match column {
            0 => format!("+{} ms", step.started_ms.saturating_sub(self.started_ms)),
            1 => match step.kind.as_str() {
                "tool_ok" => "Tool finished".into(),
                "tool_error" => "Tool failed".into(),
                "tool_running" => "Tool running".into(),
                "tool_cancelled" => "Tool cancelled".into(),
                "output" => "Response".into(),
                other => other.into(),
            },
            2 => step.name.clone(),
            3 => step
                .duration_ms
                .map(|ms| format!("{ms} ms"))
                .unwrap_or_default(),
            _ => super::summary(&step.detail, 120),
        }
    }
}
