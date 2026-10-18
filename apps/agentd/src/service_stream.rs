use crate::{Daemon, service::db_error};
use agent_protocol::v1;
use futures::Stream;
use std::{pin::Pin, sync::Arc};
use tonic::Status;
pub type EventStream = Pin<Box<dyn Stream<Item = Result<v1::Event, Status>> + Send + 'static>>;
impl Daemon {
  pub(crate) fn event_stream(&self, request: v1::WatchRequest) -> Result<EventStream, Status> {
    self
      .store
      .run(&request.run_id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("run not found"))?;
    let daemon = self.clone();
    Ok(Box::pin(async_stream::try_stream! {
      let mut cursor=request.after_cursor;
      loop {
        let notify:Arc<tokio::sync::Notify>=daemon.events.clone();let notified=notify.notified();tokio::pin!(notified);notified.as_mut().enable();
        let events=daemon.store.events(&request.run_id,cursor,256).map_err(db_error)?;
        let count=events.len();
        for event in events {cursor=event.cursor;yield event.into();}
        if count==256{continue;}
        let run=daemon.store.run(&request.run_id).map_err(db_error)?.ok_or_else(||Status::not_found("run not found"))?;
        if matches!(run.status.as_str(),"completed"|"failed"|"cancelled"|"uncertain"){break;}
        tokio::select! {_=&mut notified=>{},_=daemon.stopping.cancelled()=>break}
      }
    }))
  }
}
