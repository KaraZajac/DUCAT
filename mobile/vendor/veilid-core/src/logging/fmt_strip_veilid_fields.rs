use super::*;
use tracing_subscriber::{
    field::{MakeVisitor, RecordFields, Visit, VisitOutput},
    fmt::{
        format::{DefaultFields, Writer},
        FormatFields,
    },
};

/// A filter for the `fmt` tracing layer that can be used to remove the Veilid-specific fields from the output
/// Useful for getting the veilid-internal `__VEILID_LOG_KEY` fields gone from layers other than `ApiTracingLayer`
/// `tracing` fields can not be removed by tracing filters. They can only enable and disable
/// events and spans. To remove fields requires plugging in at the layer separately, and this struct
/// is intended for that purpose to reduce the noise veilid-core's logging facilities produce.
///
/// Example:
///
/// ```rust,no_run
/// # use veilid_core::{*, tracing_subscriber::{fmt, Layer, layer::SubscriberExt, util::SubscriberInitExt}};
/// let filter = VeilidLayerFilter::default();
/// let layer = fmt::Layer::new()
///     .compact()
///     .map_fmt_fields(FmtStripVeilidFields::mapper())
///     .with_ansi(true)
///     .with_writer(std::io::stdout)
///     .with_filter(filter.clone());
/// let subscriber = tracing_subscriber::Registry::default().with(layer);
/// subscriber.try_init().expect("logs failed to initialize");
/// ```
#[derive(Debug)]
#[must_use]
pub struct FmtStripVeilidFields {
    /// The inner formatter that will be used to format fields
    fmt: DefaultFields,
}

impl FmtStripVeilidFields {
    /// Produces a default mapping closure to pass to `fmt::Layer::new().map_fmt_fields`
    #[must_use]
    pub fn mapper() -> Box<dyn FnOnce(DefaultFields) -> FmtStripVeilidFields> {
        Box::new(|fmt| FmtStripVeilidFields { fmt })
    }
}

impl<'writer> FormatFields<'writer> for FmtStripVeilidFields {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result {
        let mut visitor = FmtStripVisitor::new(self.fmt.make_visitor(writer));
        fields.record(&mut visitor);
        visitor.finish()
    }
}

struct FmtStripVisitor<'a, F, Out>
where
    F: Visit + VisitOutput<Out>,
{
    visitor: F,
    _phantom: core::marker::PhantomData<&'a Out>,
}

impl<F, Out> FmtStripVisitor<'_, F, Out>
where
    F: Visit + VisitOutput<Out>,
{
    pub fn new(visitor: F) -> Self {
        Self {
            visitor,
            _phantom: core::marker::PhantomData {},
        }
    }

    fn strip(field: &str) -> bool {
        field == VEILID_LOG_KEY_FIELD
    }
}

impl<F, Out> VisitOutput<Out> for FmtStripVisitor<'_, F, Out>
where
    F: Visit + VisitOutput<Out>,
{
    fn finish(self) -> Out {
        self.visitor.finish()
    }

    fn visit<R>(self, fields: &R) -> Out
    where
        R: RecordFields,
        Self: Sized,
    {
        self.visitor.visit(fields)
    }
}

impl<F, Out> Visit for FmtStripVisitor<'_, F, Out>
where
    F: Visit + VisitOutput<Out>,
{
    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_debug(field, value);
    }

    fn record_f64(&mut self, field: &field::Field, value: f64) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_f64(field, value);
    }

    fn record_i64(&mut self, field: &field::Field, value: i64) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_i64(field, value);
    }

    fn record_u64(&mut self, field: &field::Field, value: u64) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_u64(field, value);
    }

    fn record_i128(&mut self, field: &field::Field, value: i128) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_i128(field, value);
    }

    fn record_u128(&mut self, field: &field::Field, value: u128) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_u128(field, value);
    }

    fn record_bool(&mut self, field: &field::Field, value: bool) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_bool(field, value);
    }

    fn record_str(&mut self, field: &field::Field, value: &str) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_str(field, value);
    }

    fn record_bytes(&mut self, field: &field::Field, value: &[u8]) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_bytes(field, value);
    }

    fn record_error(&mut self, field: &field::Field, value: &(dyn std::error::Error + 'static)) {
        if Self::strip(field.name()) {
            return;
        }
        self.visitor.record_error(field, value);
    }
}
