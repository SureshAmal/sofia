use gpui_kit::component::table::*;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sofia_content::ui::{ColorIntent, ComponentVariant, TrendDirection, UiNode};

pub fn color_from_intent(intent: &ColorIntent, cx: &gpui::App) -> gpui::Hsla {
    match intent {
        ColorIntent::Default => cx.theme().foreground,
        ColorIntent::Primary => cx.theme().primary,
        ColorIntent::Secondary => cx.theme().secondary_foreground,
        ColorIntent::Accent => cx.theme().accent_foreground,
        ColorIntent::Muted => cx.theme().muted_foreground,
        ColorIntent::Success => cx.theme().green,
        ColorIntent::Warning => cx.theme().yellow,
        ColorIntent::Danger => cx.theme().red,
    }
}

pub fn render_generative_node(node: &UiNode, cx: &gpui::App) -> AnyElement {
    render_generative_node_with_depth(node, cx, 0)
}

fn render_generative_node_with_depth(node: &UiNode, cx: &gpui::App, depth: usize) -> AnyElement {
    const MAX_TREE_DEPTH: usize = 12;
    if depth > MAX_TREE_DEPTH {
        return div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("[Nested content limit reached]")
            .into_any_element();
    }

    match node {
        UiNode::Column { children, gap } => {
            let mut col = div().flex().flex_col().w_full();
            if let Some(g) = gap {
                col = col.gap(px(*g));
            } else {
                col = col.gap_2();
            }
            for child in children {
                col = col.child(render_generative_node_with_depth(child, cx, depth + 1));
            }
            col.into_any_element()
        }
        UiNode::Row {
            children,
            gap,
            align,
        } => {
            let mut row = div().flex().flex_row().w_full();
            if let Some(g) = gap {
                row = row.gap(px(*g));
            } else {
                row = row.gap_2();
            }
            if let Some(a) = align {
                match a.as_str() {
                    "start" => row = row.items_start(),
                    "center" => row = row.items_center(),
                    "end" => row = row.items_end(),
                    "between" => row = row.justify_between(),
                    _ => row = row.items_center(),
                }
            } else {
                row = row.items_center();
            }
            for child in children {
                row = row.child(render_generative_node_with_depth(child, cx, depth + 1));
            }
            row.into_any_element()
        }
        UiNode::Card {
            title,
            description,
            children,
        } => {
            let mut card = div()
                .flex()
                .flex_col()
                .w_full()
                .gap_2()
                .text_color(cx.theme().foreground);

            if let Some(t) = title {
                let mut header = div().flex().flex_col().gap_0p5();
                header = header.child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_sm()
                        .child(t.clone()),
                );
                if let Some(desc) = description {
                    header = header.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(desc.clone()),
                    );
                }
                card = card.child(header);
            }

            for child in children {
                card = card.child(render_generative_node_with_depth(child, cx, depth + 1));
            }
            card.into_any_element()
        }
        UiNode::Text {
            content,
            size,
            weight,
            intent,
            code,
        } => {
            let mut txt = div().child(content.clone());
            if let Some(s) = size {
                match s.as_str() {
                    "xs" => txt = txt.text_xs(),
                    "sm" => txt = txt.text_sm(),
                    "base" => txt = txt.text_base(),
                    "lg" => txt = txt.text_lg(),
                    "xl" => txt = txt.text_xl(),
                    _ => txt = txt.text_sm(),
                }
            } else {
                txt = txt.text_sm();
            }
            if let Some(w) = weight {
                match w.as_str() {
                    "bold" => txt = txt.font_weight(FontWeight::BOLD),
                    "semibold" => txt = txt.font_weight(FontWeight::SEMIBOLD),
                    "medium" => txt = txt.font_weight(FontWeight::MEDIUM),
                    _ => {}
                }
            }
            if *code {
                txt = txt
                    .p_1()
                    .rounded_sm()
                    .bg(cx.theme().muted.opacity(0.35))
                    .font_family(cx.theme().font_family.clone());
            }
            let color = color_from_intent(intent, cx);
            txt.text_color(color).into_any_element()
        }
        UiNode::Badge {
            label,
            variant,
            intent,
        } => {
            let color = color_from_intent(intent, cx);
            let mut badge = div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .items_center()
                .child(label.clone());

            match variant {
                ComponentVariant::Default | ComponentVariant::Primary => {
                    badge = badge.bg(color.opacity(0.15)).text_color(color);
                }
                ComponentVariant::Secondary => {
                    badge = badge
                        .bg(cx.theme().secondary)
                        .text_color(cx.theme().secondary_foreground);
                }
                ComponentVariant::Outline => {
                    badge = badge
                        .border_1()
                        .border_color(color)
                        .text_color(color)
                        .bg(gpui::transparent_black());
                }
                ComponentVariant::Ghost => {
                    badge = badge.text_color(color);
                }
                ComponentVariant::Destructive => {
                    badge = badge
                        .bg(cx.theme().red.opacity(0.15))
                        .text_color(cx.theme().red);
                }
            }
            badge.into_any_element()
        }
        UiNode::Button {
            id: _,
            label,
            variant,
            disabled,
        } => {
            let mut btn = div()
                .px_3()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .flex()
                .items_center()
                .justify_center()
                .child(label.clone());

            if *disabled {
                btn = btn
                    .bg(cx.theme().muted.opacity(0.3))
                    .text_color(cx.theme().muted_foreground.opacity(0.5))
                    .cursor_not_allowed();
            } else {
                btn = btn.cursor_pointer();
                match variant {
                    ComponentVariant::Primary | ComponentVariant::Default => {
                        btn = btn
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .hover(|s| s.bg(cx.theme().primary.opacity(0.9)));
                    }
                    ComponentVariant::Secondary => {
                        btn = btn
                            .bg(cx.theme().secondary)
                            .text_color(cx.theme().secondary_foreground)
                            .hover(|s| s.bg(cx.theme().secondary.opacity(0.8)));
                    }
                    ComponentVariant::Outline => {
                        btn = btn
                            .border_1()
                            .border_color(cx.theme().border)
                            .text_color(cx.theme().foreground)
                            .hover(|s| s.bg(cx.theme().muted.opacity(0.2)));
                    }
                    ComponentVariant::Ghost => {
                        btn = btn
                            .text_color(cx.theme().foreground)
                            .hover(|s| s.bg(cx.theme().muted.opacity(0.2)));
                    }
                    ComponentVariant::Destructive => {
                        btn = btn
                            .bg(cx.theme().red)
                            .text_color(cx.theme().primary_foreground)
                            .hover(|s| s.bg(cx.theme().red.opacity(0.9)));
                    }
                }
            }
            btn.into_any_element()
        }
        UiNode::Switch {
            id: _,
            label,
            checked,
            disabled: _,
        } => {
            let mut track = div()
                .w(px(34.0))
                .h(px(18.0))
                .rounded_full()
                .p(px(2.0))
                .flex()
                .items_center();
            let thumb = div().w(px(14.0)).h(px(14.0)).rounded_full();

            if *checked {
                track = track
                    .bg(cx.theme().primary)
                    .justify_end()
                    .child(thumb.bg(cx.theme().primary_foreground));
            } else {
                track = track
                    .bg(cx.theme().muted)
                    .justify_start()
                    .child(thumb.bg(cx.theme().background));
            }

            let row = div()
                .flex()
                .items_center()
                .gap_2()
                .child(track)
                .child(div().text_sm().child(label.clone()));
            row.into_any_element()
        }
        UiNode::Checkbox {
            id: _,
            label,
            checked,
            disabled: _,
        } => {
            let box_el = div()
                .w(px(16.0))
                .h(px(16.0))
                .rounded_sm()
                .border_1()
                .border_color(if *checked {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .bg(if *checked {
                    cx.theme().primary
                } else {
                    cx.theme().background
                })
                .flex()
                .items_center()
                .justify_center()
                .when(*checked, |this| {
                    this.text_xs()
                        .text_color(cx.theme().primary_foreground)
                        .child("✓")
                });

            div()
                .flex()
                .items_center()
                .gap_2()
                .child(box_el)
                .child(div().text_sm().child(label.clone()))
                .into_any_element()
        }
        UiNode::Progress {
            value,
            label,
            intent,
        } => {
            let pct = (value * 100.0).clamp(0.0, 100.0);
            let bar_color = color_from_intent(intent, cx);

            let mut container = div().flex().flex_col().w_full().gap_1();
            if let Some(lbl) = label {
                container = container.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_xs()
                        .child(lbl.clone())
                        .child(format!("{:.0}%", pct)),
                );
            }
            container = container.child(
                div()
                    .w_full()
                    .h(px(6.0))
                    .rounded_full()
                    .bg(cx.theme().muted.opacity(0.3))
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(gpui::DefiniteLength::Fraction(pct / 100.0))
                            .rounded_full()
                            .bg(bar_color),
                    ),
            );
            container.into_any_element()
        }
        UiNode::Metric {
            label,
            value,
            delta,
            trend,
        } => {
            let mut metric_col = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.clone()),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::BOLD)
                        .text_color(cx.theme().foreground)
                        .child(value.clone()),
                );

            if let Some(d) = delta {
                let (trend_icon, trend_color) = match trend {
                    TrendDirection::Up => ("↑ ", cx.theme().green),
                    TrendDirection::Down => ("↓ ", cx.theme().red),
                    TrendDirection::Neutral => ("", cx.theme().muted_foreground),
                };
                metric_col = metric_col.child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(trend_color)
                        .child(format!("{trend_icon}{d}")),
                );
            }
            metric_col.into_any_element()
        }
        UiNode::KeyValue { items } => {
            let mut col = div().flex().flex_col().w_full().gap_1();
            for (k, v) in items {
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .py_0p5()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(k.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground)
                                .child(v.clone()),
                        ),
                );
            }
            col.into_any_element()
        }
        UiNode::Table { headers, rows } => {
            let mut header_row = TableRow::new();
            for h in headers {
                header_row = header_row.child(
                    TableHead::new()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_sm()
                                .truncate()
                                .child(h.clone()),
                        ),
                );
            }

            let mut body = TableBody::new();
            for row_data in rows {
                let mut data_row = TableRow::new();
                for cell in row_data {
                    data_row = data_row.child(
                        TableCell::new().child(
                            div()
                                .text_sm()
                                .overflow_hidden()
                                .child(cell.clone()),
                        ),
                    );
                }
                body = body.child(data_row);
            }

            Table::new()
                .small()
                .w_full()
                .bg(gpui::transparent_black())
                .child(TableHeader::new().child(header_row))
                .child(body)
                .into_any_element()
        }
        UiNode::Divider { vertical } => {
            if *vertical {
                div()
                    .h_full()
                    .w(px(1.0))
                    .mx_2()
                    .bg(cx.theme().border.opacity(0.5))
                    .into_any_element()
            } else {
                div()
                    .w_full()
                    .h(px(1.0))
                    .my_1()
                    .bg(cx.theme().border.opacity(0.5))
                    .into_any_element()
            }
        }
        UiNode::Accordion {
            title,
            default_open: _,
            children,
        } => {
            let mut acc = div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.clone()),
                );
            for child in children {
                acc = acc.child(render_generative_node_with_depth(child, cx, depth + 1));
            }
            acc.into_any_element()
        }
    }
}
