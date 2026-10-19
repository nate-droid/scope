//! Downstream example provider, not an existing production Scope operation.
use scope_ide::{
    document::session::DocumentSnapshot,
    tool::{ToolArguments, ToolEffect, ToolValue, operations::*},
};

pub fn object(
    fields: impl IntoIterator<Item = (&'static str, OperationSchema)>,
) -> OperationSchema {
    let fields: std::collections::BTreeMap<_, _> = fields
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    OperationSchema::Object {
        required: fields.keys().cloned().collect(),
        fields,
    }
}

pub struct InspectionProvider {
    descriptors: Vec<OperationDescriptor>,
}
impl InspectionProvider {
    pub fn new() -> Self {
        Self {
            descriptors: vec![OperationDescriptor {
                action_id: "inspect".into(),
                description: "Inspect the host-selected document snapshot".into(),
                input: object([(
                    "options",
                    object([("include_text", OperationSchema::Boolean)]),
                )]),
                output: object([
                    ("text", OperationSchema::String),
                    ("bytes", OperationSchema::Integer),
                ]),
                effect: ToolEffect::Read,
                execution: OperationExecution::Immediate,
                authorization: ["inspect_source".into()].into(),
            }],
        }
    }
}
impl OperationProvider for InspectionProvider {
    fn provider_id(&self) -> &str {
        "example_inspection"
    }
    fn descriptors(&self) -> &[OperationDescriptor] {
        &self.descriptors
    }
    fn invoke(
        &self,
        action: &str,
        snapshot: &DocumentSnapshot,
        arguments: &ToolArguments,
    ) -> Result<ToolValue, OperationFailure> {
        assert_eq!(action, "inspect");
        let ToolValue::Object(options) = &arguments["options"] else {
            unreachable!("registry validates input before dispatch")
        };
        Ok(ToolValue::object([
            (
                "text",
                ToolValue::String(if options["include_text"] == ToolValue::Boolean(true) {
                    snapshot.text.clone()
                } else {
                    String::new()
                }),
            ),
            ("bytes", ToolValue::Integer(snapshot.text.len() as i64)),
        ]))
    }
}
