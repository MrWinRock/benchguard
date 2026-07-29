use super::{Report, ReportRenderer};

pub struct JsonRenderer;

impl ReportRenderer for JsonRenderer {
    fn render(&self, report: &Report) -> String {
        serde_json::to_string(report).expect("report contains only JSON-compatible values")
    }
}
