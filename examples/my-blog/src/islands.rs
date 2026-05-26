// Islands module
//! Interactive components that hydrate on the client

use serde_json::Value;

/// Island metadata
#[derive(Debug, Clone)]
pub struct Island {
    pub name: String,
    pub props_type: Option<String>,
}

/// All registered islands
pub fn islands() -> Vec<Island> {
    vec![
        Island {
            name: "Counter".to_string(),
            props_type: Some("CounterProps".to_string()),
        },
        Island {
            name: "TodoList".to_string(),
            props_type: Some("TodoListProps".to_string()),
        },
    ]
}
