use gpui_kit::component::ActiveTheme;
use gpui_kit::component::chart::{AreaChart, BarChart, LineChart, PieChart, RadarChart};
use gpui_kit::*;
use sofia_content::{ChartPoint, ChartType};

pub fn render_chart(
    chart_id: SharedString,
    chart_type: &ChartType,
    points: &[ChartPoint],
    cx: &gpui::App,
) -> AnyElement {
    let labels = points
        .iter()
        .map(|point| point.label.clone())
        .collect::<Vec<_>>();
    let colors = [
        cx.theme().red,
        cx.theme().yellow,
        cx.theme().green,
        cx.theme().cyan,
        cx.theme().blue,
        cx.theme().magenta,
    ];
    match chart_type {
        ChartType::Line => LineChart::new(points.to_vec())
            .id(chart_id)
            .appear(false)
            .x(|point: &ChartPoint| point.label.clone())
            .y(|point: &ChartPoint| point.value)
            .stroke(cx.theme().blue)
            .dot()
            .y_axis(true)
            .into_any_element(),
        ChartType::Bar => BarChart::new(points.to_vec())
            .id(chart_id)
            .appear(false)
            .band(|point: &ChartPoint| point.label.clone())
            .value(|point: &ChartPoint| point.value)
            .fill(move |point: &ChartPoint, _, _, _| {
                colors[labels
                    .iter()
                    .position(|label| label == &point.label)
                    .unwrap_or(0)
                    % colors.len()]
            })
            .into_any_element(),
        ChartType::Area => AreaChart::new(points.to_vec())
            .id(chart_id)
            .appear(false)
            .x(|point: &ChartPoint| point.label.clone())
            .y(|point: &ChartPoint| point.value)
            .stroke(cx.theme().blue)
            .fill(cx.theme().blue.opacity(0.2))
            .into_any_element(),
        ChartType::Pie => PieChart::new(points.to_vec())
            .id(chart_id)
            .appear(false)
            .value(|point: &ChartPoint| point.value as f32)
            .label(|point: &ChartPoint| point.label.clone().into())
            .color(move |point: &ChartPoint| {
                colors[labels
                    .iter()
                    .position(|label| label == &point.label)
                    .unwrap_or(0)
                    % colors.len()]
            })
            .into_any_element(),
        ChartType::Radar => RadarChart::new(points.to_vec())
            .id(chart_id)
            .appear(false)
            .label(|point: &ChartPoint| point.label.clone())
            .value(|point: &ChartPoint| point.value)
            .stroke(cx.theme().blue)
            .fill(cx.theme().blue.opacity(0.2))
            .dot()
            .into_any_element(),
    }
}
