use crate::alarm::Status1;
use crate::alarm::Status2;
use crate::tui::app::App;
use crate::tui::app::Tab;
use chrono::DateTime;
use chrono::Local;
use chrono::TimeZone;
use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::symbols::Marker;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Axis;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Chart;
use ratatui::widgets::Dataset;
use ratatui::widgets::GraphType;
use ratatui::widgets::List;
use ratatui::widgets::ListItem;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Tabs;
use ratatui_macros::line;
use ratatui_macros::span;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

type ChartDataPoints = Vec<(f64, f64)>;

const LABEL: Style = Style::new().add_modifier(Modifier::DIM);
const BOLD: Style = Style::new().add_modifier(Modifier::BOLD);

fn soc_bar(soc: f32, width: usize) -> String {
    let soc = soc.clamp(0.0, 100.0);
    let filled = ((soc / 100.0) * width as f32) as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn min_max(values: &[f32]) -> Option<(f32, f32)> {
    let min = values.iter().copied().reduce(f32::min)?;
    let max = values.iter().copied().reduce(f32::max)?;
    Some((min, max))
}

fn color_current(amps: f32) -> Color {
    if amps >= 0.0 {
        Color::Green
    } else {
        Color::Yellow
    }
}

fn color_soc(soc: f32) -> Color {
    if soc >= 50.0 {
        Color::Green
    } else if soc >= 20.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(14),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_tab_bar(frame, app, chunks[0]);

    match app.active_tab {
        Tab::Overview => draw_overview(frame, app, chunks[1]),
        Tab::Graphs => draw_graphs(frame, app, chunks[1]),
    }

    draw_status_bar(frame, app, chunks[2]);
}

fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let titles = vec!["Overview", "Graphs"];
    let selected = match app.active_tab {
        Tab::Overview => 0,
        Tab::Graphs => 1,
    };

    let tabs = Tabs::new(titles)
        .select(selected)
        .style(LABEL)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider("|");

    frame.render_widget(tabs, area);
}

fn draw_overview(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(8)])
        .split(area);

    draw_rollup(frame, app, chunks[0]);
    draw_main_area(frame, app, chunks[1]);
}

fn draw_rollup(frame: &mut Frame, app: &App, area: Rect) {
    let summary = app.summary();

    let temp_str = match summary.average_temperature {
        Some(temp) => format!("{:.1}C", temp),
        None => "N/A".to_string(),
    };

    let sign = if summary.total_current >= 0.0 {
        "+"
    } else {
        ""
    };
    let soc = summary.average_soc;
    let bar = soc_bar(soc, 40);

    let alarm_count = app
        .batteries
        .iter()
        .filter(|(_, info)| info.as_ref().is_some_and(|b| b.has_alarms()))
        .count();

    let mut first_line = line![
        span!(LABEL; "Current: "),
        span!(Style::default().fg(color_current(summary.total_current)); format!("{sign}{:.1}A", summary.total_current)),
        "    ",
        span!(LABEL; "Capacity: "),
        format!(
            "{:.0}/{:.0}Ah",
            summary.total_remaining_ah, summary.total_capacity_ah
        ),
        "    ",
        span!(LABEL; "Temp: "),
        span!(Style::default().fg(Color::Cyan); temp_str),
    ];

    if alarm_count > 0 {
        first_line.push_span(Span::raw("    "));
        first_line.push_span(
            span!(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
            format!("ALARMS: {}", alarm_count)),
        );
    }

    let lines = vec![
        first_line,
        line![],
        line![
            span!(LABEL; "SOC: "),
            span!(Style::default().fg(color_soc(soc)); format!("{:5.1}% ", soc)),
            span!(Style::default().fg(color_soc(soc)); bar),
        ],
    ];

    let title = if summary.battery_count == 1 {
        " Summary (1 battery) ".to_string()
    } else {
        format!(" Summary ({} batteries) ", summary.battery_count)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(BOLD);

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_main_area(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(42), Constraint::Min(40)])
        .split(area);

    draw_battery_list(frame, app, chunks[0]);
    draw_battery_detail(frame, app, chunks[1]);
}

fn draw_battery_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .batteries
        .iter()
        .map(|(addr, info)| {
            let Some(b) = info else {
                return ListItem::new(format!("0x{:02X} ---", addr)).style(LABEL);
            };

            let has_alarm = b.has_alarms();
            let alarm_indicator = if has_alarm { "!" } else { " " };

            let content = Line::from(vec![
                span!(if has_alarm { Style::default().fg(Color::Red) } else { Style::default() };
                      alarm_indicator),
                Span::raw(format!(
                    "{} {:4.1}% {:.1}V",
                    b.serial, b.soc_percent, b.module_voltage
                )),
            ]);

            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Batteries ")
                .title_style(BOLD),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_battery_detail(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Battery Details ")
        .title_style(BOLD);

    let Some(battery) = app.selected_battery() else {
        let addr = app
            .batteries
            .get(app.selected())
            .map(|(a, _)| *a)
            .unwrap_or(0);
        frame.render_widget(
            Paragraph::new(format!("No data for 0x{:02X}", addr)).block(block),
            area,
        );
        return;
    };

    let sign = if battery.current >= 0.0 { "+" } else { "" };
    let soc = battery.soc_percent;
    let bar = soc_bar(soc, 40);

    let mut lines: Vec<Line> = vec![
        line![
            span!(BOLD; &battery.model),
            if battery.model.is_empty() { "" } else { "  " },
            span!(LABEL; "SN: "),
            Span::raw(&battery.serial),
            "  ",
            Span::raw(&battery.software_version),
        ],
        line![],
        line![
            span!(LABEL; "Voltage: "),
            span!(Style::default().fg(Color::Cyan); format!("{:.2}V", battery.module_voltage)),
            "    ",
            span!(LABEL; "Current: "),
            span!(Style::default().fg(color_current(battery.current)); format!("{sign}{:.2}A", battery.current)),
            "    ",
            span!(LABEL; "Cycles: "),
            format!("{}", battery.cycle_count),
        ],
        line![
            span!(LABEL; "Capacity: "),
            format!(
                "{:.1}/{:.1}Ah",
                battery.remaining_capacity, battery.total_capacity
            ),
        ],
        line![],
        line![
            span!(LABEL; "SOC: "),
            span!(Style::default().fg(color_soc(soc)); format!("{:5.1}% ", soc)),
            span!(Style::default().fg(color_soc(soc)); bar),
        ],
        line![],
    ];

    // Temperatures
    if let Some((min_t, max_t)) = min_max(&battery.cell_temperatures) {
        lines.push(line![
            span!(LABEL; "Temp: "),
            span!(Style::default().fg(Color::Cyan); format!("{:.1}-{:.1}C", min_t, max_t)),
            span!(LABEL; format!(" ({} sensors)", battery.cell_temperatures.len())),
        ]);
    } else {
        lines.push(line![span!(LABEL; "Temp: "), "(no data)"]);
    }

    lines.push(line![]);

    // Cell voltages
    if let Some((min_v, max_v)) = min_max(&battery.cell_voltages) {
        let delta = max_v - min_v;
        lines.push(line![
            span!(LABEL; format!("Cells[{}]: ", battery.cell_voltages.len())),
            span!(Style::default().fg(Color::Red); format!("{:.3}", min_v)),
            "-",
            span!(Style::default().fg(Color::Green); format!("{:.3}V", max_v)),
            span!(LABEL; format!(" Δ{:3.0}mV", delta * 1000.0)),
        ]);

        for (i, chunk) in battery.cell_voltages.chunks(4).enumerate() {
            let row_start = i * 4 + 1;
            let mut spans: Vec<Span> = vec![span!(LABEL; format!(" {:>2}: ", row_start))];
            for &v in chunk {
                let style = if delta > 0.005 && (v - min_v).abs() < 0.001 {
                    Style::default().fg(Color::Red)
                } else if delta > 0.005 && (v - max_v).abs() < 0.001 {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };
                spans.push(span!(style; format!("{:.3}  ", v)));
            }
            lines.push(Line::from(spans));
        }
    } else {
        lines.push(line![
            span!(LABEL; format!("Cells[{}]: ", battery.cell_count)),
            "(no voltage data)",
        ]);
    }

    lines.push(line![]);

    // Other temperatures (environment, heater)
    let mut other_temps: Vec<String> = Vec::new();
    for (i, &t) in battery.environment_temperatures.iter().enumerate() {
        other_temps.push(format!("Env{}: {:.1}C", i + 1, t));
    }
    for (i, &t) in battery.heater_temperatures.iter().enumerate() {
        other_temps.push(format!("Htr{}: {:.1}C", i + 1, t));
    }
    if !other_temps.is_empty() {
        lines.push(line![span!(LABEL; "Other Temps: "), other_temps.join("  "),]);
    }

    // Limits
    if let (Some(cv), Some(dv)) = (
        battery.charge_voltage_limit,
        battery.discharge_voltage_limit,
    ) {
        lines.push(line![
            span!(LABEL; "Limits: "),
            format!(
                "V: {:.1}-{:.1}V  I: {:.1}/{:.1}A",
                dv,
                cv,
                battery.charge_current_limit.unwrap_or(0.0),
                battery.discharge_current_limit.unwrap_or(0.0)
            ),
        ]);
    }

    // MOSFET and status
    if let Some(s1) = battery.status1 {
        let charge_on = s1.contains(Status1::CHARGE_MOSFET);
        let discharge_on = s1.contains(Status1::DISCHARGE_MOSFET);
        lines.push(line![
            span!(LABEL; "MOSFETs: "),
            span!(if charge_on { Style::default().fg(Color::Green) } else { LABEL };
                  format!("Chg:{}", if charge_on { "ON" } else { "off" })),
            "  ",
            span!(if discharge_on { Style::default().fg(Color::Green) } else { LABEL };
                  format!("Dis:{}", if discharge_on { "ON" } else { "off" })),
        ]);
    }

    // Status indicators
    if let Some(s2) = battery.status2 {
        let mut status_items = Vec::new();
        if s2.contains(Status2::FULLY_CHARGED) {
            status_items.push(("FULL", Color::Green));
        }
        if s2.contains(Status2::HEATER_ON) {
            status_items.push(("HEATER", Color::Yellow));
        }
        if !status_items.is_empty() {
            let mut spans: Vec<Span> = vec![span!(LABEL; "State: ")];
            for (label, color) in status_items {
                spans.push(span!(Style::default().fg(color); label));
                spans.push(Span::raw(" "));
            }
            lines.push(Line::from(spans));
        }
    }

    // Alarms
    let alarms = battery.active_alarms();
    if !alarms.is_empty() {
        lines.push(line![]);
        lines.push(line![
            span!(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD); "ALARMS:")
        ]);
        for alarm in alarms {
            lines.push(line![
                span!(Style::default().fg(Color::Red); format!("  {}", alarm)),
            ]);
        }
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let last_update = app
        .last_update
        .map(|t| {
            let secs = t.elapsed().as_secs();
            if secs < 60 {
                format!("{}s", secs)
            } else {
                format!("{}m", secs / 60)
            }
        })
        .unwrap_or_else(|| "-".to_string());

    let status = if app.refreshing {
        span!(Style::default().fg(Color::Yellow); "Refreshing")
    } else if app.error.is_some() {
        span!(Style::default().fg(Color::Red); "Error")
    } else {
        span!(Style::default().fg(Color::Green); "OK")
    };

    let tab_hints = match app.active_tab {
        Tab::Overview => line![
            span!(LABEL; " q"),
            ":quit ",
            span!(LABEL; "Tab"),
            ":graphs ",
            span!(LABEL; "jk"),
            ":sel ",
            span!(LABEL; "r"),
            ":refresh | ",
            status,
            format!(" | {}", last_update),
        ],
        Tab::Graphs => line![
            span!(LABEL; " q"),
            ":quit ",
            span!(LABEL; "Tab"),
            ":overview ",
            span!(LABEL; "+-"),
            ":zoom ",
            span!(LABEL; "hl"),
            ":scroll ",
            span!(LABEL; "r"),
            ":refresh | ",
            status,
            format!(" | {} | {} pts", last_update, app.history.len()),
        ],
    };

    frame.render_widget(Paragraph::new(tab_hints), area);
}

fn draw_graphs(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(area);

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let window_secs = app.graph_view.zoom_window_secs();
    let scroll_offset = app.graph_view.scroll_offset_secs;

    let view_end = now_secs.saturating_sub(scroll_offset);
    let view_start = view_end.saturating_sub(window_secs);

    let max_points = (area.width as usize).saturating_mul(2);
    let (current_data, soc_data, temp_data) =
        prepare_chart_data(app, view_start, view_end, max_points);

    let current_bounds = calculate_y_bounds(&current_data, None);
    let soc_bounds = [0.0, 100.0];
    let temp_bounds = calculate_y_bounds(&temp_data, None);

    let y_label_width = [current_bounds, soc_bounds, temp_bounds]
        .iter()
        .flat_map(|b| b.iter())
        .map(|v| format!("{:.1}", v).len())
        .max()
        .unwrap_or(4);

    draw_single_chart_with_zero_line(
        frame,
        chunks[0],
        "Current (A)",
        app.graph_view.zoom_label(),
        &current_data,
        view_start,
        view_end,
        Color::Green,
        current_bounds,
        y_label_width,
        true,
    );

    draw_single_chart(
        frame,
        chunks[1],
        "SOC (%)",
        "",
        &soc_data,
        view_start,
        view_end,
        Color::Yellow,
        soc_bounds,
        y_label_width,
    );

    draw_single_chart(
        frame,
        chunks[2],
        "Temperature (°C)",
        "",
        &temp_data,
        view_start,
        view_end,
        Color::Cyan,
        temp_bounds,
        y_label_width,
    );
}

fn prepare_chart_data(
    app: &App,
    view_start: u64,
    view_end: u64,
    max_points: usize,
) -> (ChartDataPoints, ChartDataPoints, ChartDataPoints) {
    let mut current_data = Vec::new();
    let mut soc_data = Vec::new();
    let mut temp_data = Vec::new();

    for point in app.history.iter() {
        if point.timestamp_secs >= view_start && point.timestamp_secs <= view_end {
            let x = point.timestamp_secs as f64;
            current_data.push((x, point.current as f64));
            soc_data.push((x, point.soc as f64));
            if let Some(t) = point.temp_avg {
                temp_data.push((x, t as f64));
            }
        }
    }

    (
        downsample_minmax(&current_data, max_points),
        downsample_minmax(&soc_data, max_points),
        downsample_minmax(&temp_data, max_points),
    )
}

fn downsample_minmax(data: &[(f64, f64)], max_points: usize) -> ChartDataPoints {
    if data.len() <= max_points || max_points < 4 {
        return data.to_vec();
    }

    let num_buckets = max_points / 2;
    let bucket_size = data.len() / num_buckets;
    let mut result = Vec::with_capacity(max_points);

    for chunk in data.chunks(bucket_size) {
        let min = chunk.iter().min_by(|a, b| a.1.total_cmp(&b.1));
        let max = chunk.iter().max_by(|a, b| a.1.total_cmp(&b.1));

        if let (Some(&min_pt), Some(&max_pt)) = (min, max) {
            if min_pt.0 <= max_pt.0 {
                result.push(min_pt);
                if min_pt.0 != max_pt.0 {
                    result.push(max_pt);
                }
            } else {
                result.push(max_pt);
                if min_pt.0 != max_pt.0 {
                    result.push(min_pt);
                }
            }
        }
    }

    result
}

fn calculate_y_bounds(data: &[(f64, f64)], fixed_bounds: Option<(f64, f64)>) -> [f64; 2] {
    if let Some((min, max)) = fixed_bounds {
        return [min, max];
    }

    if data.is_empty() {
        return [0.0, 1.0];
    }

    let min_y = data.iter().map(|(_, y)| *y).fold(f64::MAX, f64::min);
    let max_y = data.iter().map(|(_, y)| *y).fold(f64::MIN, f64::max);

    let range = max_y - min_y;
    let padding = if range.abs() < 0.001 {
        1.0
    } else {
        range * 0.1
    };

    [min_y - padding, max_y + padding]
}

fn format_time_axis_labels(start: u64, end: u64) -> Vec<Span<'static>> {
    let duration = end.saturating_sub(start);
    let mid = start + duration / 2;

    let include_date = duration > 12 * 3600 || spans_midnight(start, end);

    vec![
        Span::raw(format_timestamp(start, include_date)),
        Span::raw(format_timestamp(mid, include_date)),
        Span::raw(format_timestamp(end, include_date)),
    ]
}

fn spans_midnight(start: u64, end: u64) -> bool {
    let start_dt: DateTime<Local> = Local.timestamp_opt(start as i64, 0).unwrap();
    let end_dt: DateTime<Local> = Local.timestamp_opt(end as i64, 0).unwrap();
    start_dt.date_naive() != end_dt.date_naive()
}

fn format_timestamp(ts: u64, include_date: bool) -> String {
    let dt: DateTime<Local> = Local.timestamp_opt(ts as i64, 0).unwrap();
    if include_date {
        dt.format("%b %d %H:%M").to_string()
    } else {
        dt.format("%H:%M").to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_single_chart(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    zoom_label: &str,
    data: &[(f64, f64)],
    view_start: u64,
    view_end: u64,
    color: Color,
    y_bounds: [f64; 2],
    y_label_width: usize,
) {
    draw_single_chart_with_zero_line(
        frame,
        area,
        title,
        zoom_label,
        data,
        view_start,
        view_end,
        color,
        y_bounds,
        y_label_width,
        false,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_single_chart_with_zero_line(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    zoom_label: &str,
    data: &[(f64, f64)],
    view_start: u64,
    view_end: u64,
    color: Color,
    y_bounds: [f64; 2],
    y_label_width: usize,
    show_zero_line: bool,
) {
    let x_labels = format_time_axis_labels(view_start, view_end);

    let block_title = if zoom_label.is_empty() {
        format!(" {} ", title)
    } else {
        format!(" {} [{}] ", title, zoom_label)
    };

    let mut datasets = Vec::new();

    let zero_line_data: Vec<(f64, f64)>;
    if show_zero_line && y_bounds[0] < 0.0 && y_bounds[1] > 0.0 {
        zero_line_data = vec![(view_start as f64, 0.0), (view_end as f64, 0.0)];
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                )
                .data(&zero_line_data),
        );
    }

    datasets.push(
        Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(color))
            .data(data),
    );

    let chart = Chart::new(datasets)
        .block(Block::default().borders(Borders::ALL).title(block_title))
        .x_axis(
            Axis::default()
                .style(LABEL)
                .bounds([view_start as f64, view_end as f64])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .style(LABEL)
                .bounds(y_bounds)
                .labels(format_y_labels(y_bounds, y_label_width)),
        );

    frame.render_widget(chart, area);
}

fn format_y_label(value: f64, width: usize) -> String {
    if value == 0.0 || value == -0.0 {
        format!("{:>width$.1}", 0.0_f64)
    } else {
        format!("{:>width$.1}", value)
    }
}

fn format_y_labels(bounds: [f64; 2], width: usize) -> Vec<Span<'static>> {
    vec![
        Span::raw(format_y_label(bounds[0], width)),
        Span::raw(format_y_label(bounds[1], width)),
    ]
}

#[cfg(test)]
mod tests {
    use super::calculate_y_bounds;
    use super::downsample_minmax;
    use super::soc_bar;

    #[test]
    fn soc_bar_fills_proportionally() {
        let bar = soc_bar(50.0, 40);
        assert_eq!(bar.chars().count(), 40);
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 20);
    }

    #[test]
    fn soc_bar_clamps_out_of_range() {
        assert_eq!(soc_bar(150.0, 10).chars().filter(|&c| c == '█').count(), 10);
        assert_eq!(soc_bar(-5.0, 10).chars().filter(|&c| c == '█').count(), 0);
    }

    #[test]
    fn downsample_keeps_small_series() {
        let data = vec![(0.0, 1.0), (1.0, 2.0), (2.0, 3.0)];
        assert_eq!(downsample_minmax(&data, 10), data);
    }

    #[test]
    fn downsample_bounds_large_series() {
        let data: Vec<(f64, f64)> = (0..1000).map(|i| (i as f64, i as f64)).collect();
        assert!(downsample_minmax(&data, 50).len() <= 50);
    }

    #[test]
    fn y_bounds_default_for_empty() {
        assert_eq!(calculate_y_bounds(&[], None), [0.0, 1.0]);
    }

    #[test]
    fn y_bounds_honor_fixed() {
        assert_eq!(
            calculate_y_bounds(&[(0.0, 5.0)], Some((0.0, 100.0))),
            [0.0, 100.0]
        );
    }

    #[test]
    fn y_bounds_pad_data_range() {
        let bounds = calculate_y_bounds(&[(0.0, 1.0), (1.0, 3.0)], None);
        assert!(bounds[0] < 1.0 && bounds[1] > 3.0);
    }
}
