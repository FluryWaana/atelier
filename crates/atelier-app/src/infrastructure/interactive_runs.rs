use super::{
    command_runs::{Reply, lines},
    database::Database,
    process_tree::ContainedChild,
    runs,
    store::Documents,
};
use crate::Result;
use atelier_core::{
    application::agent_protocol::{Effects, Protocol},
    document::Document,
};
use std::time::Duration;
use tokio::{
    io::AsyncWriteExt,
    process::{ChildStderr, ChildStdin, ChildStdout},
    sync::{broadcast, mpsc, watch},
};

pub(super) struct Session<'a> {
    pub database: &'a Database,
    pub id: &'a str,
    pub child: ContainedChild,
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub stderr: ChildStderr,
    pub protocol: Protocol,
    pub cancel: watch::Receiver<bool>,
    pub replies: mpsc::Receiver<Reply>,
    pub changed: &'a broadcast::Sender<()>,
}

#[derive(Debug)]
struct ProtocolFailure(&'static str);
impl std::fmt::Display for ProtocolFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for ProtocolFailure {}

async fn apply(
    database: &Database,
    id: &str,
    stdin: &mut ChildStdin,
    protocol: &Protocol,
    effects: Effects,
    changed: &broadcast::Sender<()>,
) -> Result<Option<bool>> {
    for message in effects.send {
        let mut bytes = serde_json::to_vec(&message)?;
        if bytes.len() > 512 * 1024 {
            return Err("message.cli_message_too_large".into());
        }
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(5), stdin.write_all(&bytes))
            .await
            .map_err(|_| "message.cli_no_longer_receives_messages")??;
    }
    let interactions = protocol.interactions();
    let waiting = protocol.waiting();
    let id = id.to_owned();
    let completed = effects.complete;
    database
        .call(move |store| {
            for text in effects.text {
                runs::append(store, &id, "stdout", &text)?;
            }
            let mut run = store
                .document::<Document>(Documents::Runs, &id)?
                .ok_or("message.run_not_found")?;
            run.set("interactions", &interactions)?;
            run.set("waiting", &waiting)?;
            if let Some(summary) = effects.summary {
                runs::record_summary(&mut run, &summary)?;
            }
            if let Some(usage) = effects.usage {
                run.set("usage", &usage)?;
            }
            if let Some(preview) = effects.preview {
                run.set("liveText", &preview)?;
            }
            store.put(Documents::Runs, &run)
        })
        .await?;
    let _ = changed.send(());
    Ok(completed)
}

pub(super) async fn drive(session: Session<'_>) -> Result<()> {
    let Session {
        database,
        id,
        mut child,
        stdin,
        stdout,
        stderr,
        mut protocol,
        mut cancel,
        mut replies,
        changed,
    } = session;
    let mut input = Some(stdin);
    let (sender, mut output) = mpsc::channel(16);
    let mut stdout_task = tokio::spawn(lines(stdout, "stdout", sender.clone()));
    let mut stderr_task = tokio::spawn(lines(stderr, "stderr", sender));
    let mut stdout_done = false;
    let mut stderr_done = false;
    let mut exited = false;
    let mut complete = None;
    let mut cancelled = false;
    let mut active = Duration::ZERO;
    let beginning = tokio::time::Instant::now();
    let mut closing = None::<tokio::time::Instant>;
    let mut preview_at = tokio::time::Instant::now();
    let operation: Result<()> = async {
        let effects=protocol.start().map_err(ProtocolFailure)?;
        apply(database,id,input.as_mut().ok_or("message.channel_closed")?,&protocol,effects,changed).await?;
        loop {
            if *cancel.borrow() { cancelled=true; break; }
            if exited && stdout_done && stderr_done && output.is_empty() {
                if complete.is_none() { return Err("message.cli_stopped_without_result".into()); }
                break;
            }
            let waiting=protocol.waiting();
            let began=tokio::time::Instant::now();
            let work=if waiting { Duration::from_secs(24*3600) } else { Duration::from_secs(3600).saturating_sub(active) };
            let wall=Duration::from_secs(24*3600).saturating_sub(beginning.elapsed());
            let cleanup=closing.map(|deadline| deadline.saturating_duration_since(began)).unwrap_or(wall);
            let startup=if protocol.ready(){wall}else{Duration::from_secs(30).saturating_sub(beginning.elapsed())};
            let delay=work.min(wall).min(cleanup).min(startup);
            tokio::select! {
                biased;
                _=cancel.changed()=> { cancelled=true; break; },
                _=tokio::time::sleep(delay)=>return Err("message.session_timeout".into()),
                status=child.wait(), if !exited=> {
                    status?; exited=true;
                },
                result=&mut stdout_task, if !stdout_done=> { result??; stdout_done=true; },
                result=&mut stderr_task, if !stderr_done=> { result??; stderr_done=true; },
                message=output.recv(), if !stdout_done || !stderr_done || !output.is_empty()=> {
                    if let Some((stream,line))=message {
                        if complete.is_some() { if !waiting {active=active.saturating_add(began.elapsed());} continue; }
                        let mut effects=if stream=="stdout" {
                            let event=serde_json::from_str(&line).map_err(|_| "message.protocol_output_invalid")?;
                            protocol.receive(event).map_err(ProtocolFailure)?
                        } else { Effects { text:vec!["Le CLI a écrit sur sa sortie d’erreur ; contenu masqué.".into()], ..Effects::default() } };
                        let public_change=!effects.send.is_empty() || !effects.text.is_empty() || effects.summary.is_some() || effects.usage.is_some() || effects.complete.is_some() || effects.interactions_changed;
                        if !public_change && preview_at.elapsed()<Duration::from_millis(250) { effects.preview=None; }
                        if !public_change && effects.preview.is_none() { if !waiting {active=active.saturating_add(began.elapsed());} continue; }
                        if effects.preview.is_some() { preview_at=tokio::time::Instant::now(); }
                        let result=apply(database,id,input.as_mut().ok_or("message.channel_closed")?,&protocol,effects,changed).await?;
                        if let Some(success)=result {
                            complete=Some(success);
                            drop(input.take());
                            closing=Some(tokio::time::Instant::now()+Duration::from_secs(5));
                        }
                    }
                },
                response=replies.recv(), if complete.is_none()=> {
                    let Some(response)=response else { return Err("message.reply_channel_closed".into()); };
                    match protocol.reply(response.answer) {
                        Ok(effects)=> {
                            let result=apply(database,id,input.as_mut().ok_or("message.channel_closed")?,&protocol,effects,changed).await;
                            let _=response.result.send(if result.is_ok(){Ok(())}else{Err("message.response_send_failed")});
                            result?;
                        },
                        Err(message)=> { let _=response.result.send(Err(message)); },
                    }
                },
            }
            if !waiting { active=active.saturating_add(began.elapsed()); }
        }
        Ok(())
    }.await;
    if cancelled {
        let id = id.to_owned();
        let timestamp = timestamp()?;
        database
            .call(move |store| runs::transition(store, &id, "stopping", &timestamp, None))
            .await?;
        let _ = changed.send(());
    }
    // Keep readers and their pipes alive until the owned process tree is stopped.
    // Aborting them first can turn a protocol/cancellation error into a SIGPIPE exit race.
    let stopped = if cancelled || operation.is_err() {
        child.stop().await.map(|_| ())
    } else {
        Ok(())
    };
    stdout_task.abort();
    stderr_task.abort();
    stopped?;
    let code = child.child.try_wait()?.and_then(|status| status.code());
    let id = id.to_owned();
    let timestamp = timestamp()?;
    database
        .call(move |store| {
            let attention = operation.as_ref().err().map(|error| {
                error
                    .downcast_ref::<ProtocolFailure>()
                    .map(|error| error.0)
                    .unwrap_or("message.session_stopped_after_error")
            });
            if let Some(message) = attention {
                runs::append(store, &id, "system", message)?;
            }
            let status = if cancelled {
                "cancelled"
            } else if operation.is_ok() && complete == Some(true) && code == Some(0) {
                "completed"
            } else {
                "failed"
            };
            let mut run = runs::transition(store, &id, status, &timestamp, code)?;
            if let Some(message) = attention {
                run.set("attention", &message)?;
            } else if status == "failed" {
                run.set("attention", &"message.agent_or_cli_failed")?;
            }
            run.set(
                "activeMilliseconds",
                &u64::try_from(active.as_millis()).unwrap_or(u64::MAX),
            )?;
            store.put(Documents::Runs, &run)
        })
        .await
}

fn timestamp() -> Result<String> {
    Ok(time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?)
}
