//! PR #580 evaluator, ready for the host's isolated-archive run binding.
//! No clock is needed in the guest: the host session enforces the whole-call deadline.
use serde_json::{json, Value};
use std::time::Duration;

/// Permission-filtered host run(command, args), scoped to one staged archive.
pub(crate) trait Session {
    fn run(&mut self, command: &str, args: &Value) -> Result<Value, String>;
}

pub(crate) type Execute<'a> = dyn FnMut(&mut dyn Session) -> Result<Vec<Value>, String> + 'a;

/// The adapter must open a fresh archive context, bound reads/results, enforce the
/// deadline across all commands, and release the context on success AND failure.
/// This cannot be implemented by forwarding commands to the user's active design.
pub(crate) trait IsolatedHost: std::fmt::Debug + Send + Sync {
    fn with_design(
        &self,
        design: &[u8],
        timeout: Duration,
        label: &str,
        execute: &mut Execute<'_>,
    ) -> Result<Vec<Value>, String>;
}

#[derive(Debug)]
pub(crate) struct HostEvaluator<H>(pub(crate) H);

impl<H: IsolatedHost> HostEvaluator<H> {
    pub(crate) fn evaluate(
        &self,
        design: &[u8],
        timeout: Duration,
        label: &str,
    ) -> Result<Value, String> {
        let commands = [
            json!({"command": "inspect.measure", "params": {}}),
            json!({"command": "document.inspect", "params": {}}),
        ];
        let results = self.run(design, &commands, timeout, label)?;
        // A refusal is an evaluated answer, not a transport failure.
        for result in &results {
            if result.get("evaluation_failed").is_some() {
                return Ok(result.clone());
            }
        }
        let mut results = results.into_iter();
        let measured = results.next().ok_or("Host returned no measurements")?;
        let inspected = results
            .next()
            .ok_or("Host returned no document inspection")?;
        let mut evaluated = measured
            .as_object()
            .cloned()
            .ok_or("Host measurements must be an object")?;
        if !evaluated.get("bodies").is_some_and(Value::is_array) {
            return Err("Host measurements have no bodies".into());
        }
        let timeline = inspected
            .get("timeline")
            .filter(|value| value.is_array())
            .ok_or("Host document inspection has no timeline")?;
        evaluated.insert("timeline".into(), timeline.clone());
        // Body geometry and timeline diagnostics retain the engine's raw schemas.
        Ok(Value::Object(evaluated))
    }

    pub(crate) fn run(
        &self,
        design: &[u8],
        commands: &[Value],
        timeout: Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        if design.len() > crate::MAX_OUTPUT_BYTES || commands.len() > 4096 {
            return Err("Host evaluation exceeds the design or command limit".into());
        }
        if timeout.is_zero() {
            return Err("Host evaluation deadline has elapsed".into());
        }
        let mut execute = |session: &mut dyn Session| {
            let mut results = Vec::with_capacity(commands.len());
            let mut bytes = 0usize;
            for request in commands {
                let command = request
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or("Evaluation request has no command")?;
                if !matches!(
                    command,
                    "inspect.measure"
                        | "document.inspect"
                        | "timeline.roll_to"
                        | "model.edges"
                        | "model.faces"
                ) {
                    return Err(format!("Evaluation command {command} is not supported yet"));
                }
                let args = request.get("params").cloned().unwrap_or_else(|| json!({}));
                let result = session.run(command, &args)?;
                bytes = bytes.saturating_add(
                    serde_json::to_vec(&result)
                        .map_err(|e| e.to_string())?
                        .len(),
                );
                if bytes > 64 << 20 {
                    return Err("Host evaluation results exceed 64 MiB".into());
                }
                // Preserve raw command failures in order; only transport failures abort.
                results.push(result);
            }
            Ok(results)
        };
        self.0.with_design(design, timeout, label, &mut execute)
    }
}

// TODO(eval-seam, PR #580): remove this false cfg once the trait lands, implement
// IsolatedHost in adapter.rs using the reviewed archive/session binding and its
// host run() import, then pass Arc::new(HostEvaluator(host)) to writer.evaluator().
// Do not enable verification until the host guarantees isolated, bounded sessions.
#[cfg(any())]
impl<H: IsolatedHost> dutchman_solidcraft::SolidCraftEvaluator for HostEvaluator<H> {
    fn evaluate(&self, design: &[u8], timeout: Duration, label: &str) -> Result<Value, String> {
        HostEvaluator::evaluate(self, design, timeout, label)
    }
    fn run(
        &self,
        design: &[u8],
        commands: &[Value],
        timeout: Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        HostEvaluator::run(self, design, commands, timeout, label)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct Host(Mutex<Vec<Vec<String>>>);
    impl IsolatedHost for Host {
        fn with_design(
            &self,
            design: &[u8],
            timeout: Duration,
            label: &str,
            execute: &mut Execute<'_>,
        ) -> Result<Vec<Value>, String> {
            assert_eq!(design, b"archive");
            assert_eq!(timeout, Duration::from_secs(3));
            assert_eq!(label, "proof");
            struct Context(Vec<String>);
            impl Session for Context {
                fn run(&mut self, command: &str, args: &Value) -> Result<Value, String> {
                    self.0.push(command.into());
                    Ok(match command {
                        "inspect.measure" => {
                            json!({"bodies":[{"volume_mm3":1000,"area_mm2":600,"bbox":{"min":[0,0,0],"max":[10,10,10]}}],"total":{"faces":6}})
                        }
                        "document.inspect" => {
                            json!({"timeline":[{"name":"Fillet","error":null,"warning":"mesh warning"}]})
                        }
                        "timeline.roll_to" => json!({"error":"feature refused", "args":args}),
                        _ => json!({"raw":command}),
                    })
                }
            }
            let mut context = Context(Vec::new());
            let result = execute(&mut context);
            self.0.lock().unwrap().push(context.0);
            result
        }
    }

    #[test]
    fn combines_measurements_and_diagnostics_in_one_context() {
        let evaluator = HostEvaluator(Host::default());
        let result = evaluator
            .evaluate(b"archive", Duration::from_secs(3), "proof")
            .unwrap();
        assert_eq!(result["bodies"][0]["volume_mm3"], 1000);
        assert_eq!(result["total"]["faces"], 6);
        assert_eq!(result["timeline"][0]["warning"], "mesh warning");
        assert_eq!(
            *evaluator.0 .0.lock().unwrap(),
            vec![vec!["inspect.measure", "document.inspect"]]
        );
    }

    #[test]
    fn preserves_order_args_and_raw_failures_with_fresh_context_per_call() {
        let evaluator = HostEvaluator(Host::default());
        let commands = [
            json!({"command":"timeline.roll_to","params":{"index":2}}),
            json!({"command":"model.edges","params":{"body":"Cube"}}),
            json!({"command":"model.faces","params":{"body":"Cube"}}),
        ];
        for _ in 0..2 {
            let results = evaluator
                .run(b"archive", &commands, Duration::from_secs(3), "proof")
                .unwrap();
            assert_eq!(results[0]["args"], commands[0]["params"]);
            assert_eq!(results[0]["error"], "feature refused");
            assert_eq!(results[1]["raw"], "model.edges");
            assert_eq!(results[2]["raw"], "model.faces");
        }
        assert_eq!(evaluator.0 .0.lock().unwrap().len(), 2);
        assert!(evaluator
            .run(b"archive", &commands, Duration::ZERO, "proof")
            .is_err());
    }

    #[test]
    fn evaluated_refusal_and_transport_timeout_remain_distinct() {
        #[derive(Debug)]
        struct Answer(Result<Vec<Value>, String>);
        impl IsolatedHost for Answer {
            fn with_design(
                &self,
                _: &[u8],
                _: Duration,
                _: &str,
                _: &mut Execute<'_>,
            ) -> Result<Vec<Value>, String> {
                self.0.clone()
            }
        }
        let refusal = json!({"evaluation_failed":"host refused the design"});
        let host = HostEvaluator(Answer(Ok(vec![refusal.clone()])));
        assert_eq!(
            host.evaluate(b"archive", Duration::from_secs(3), "proof")
                .unwrap(),
            refusal
        );
        let host = HostEvaluator(Answer(Err("host deadline elapsed".into())));
        assert_eq!(
            host.evaluate(b"archive", Duration::from_secs(3), "proof"),
            Err("host deadline elapsed".into())
        );
    }
}
