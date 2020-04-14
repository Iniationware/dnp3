use crate::app::format::write::start_request;
use crate::app::gen::enums::FunctionCode;
use crate::app::header::{Control, ResponseHeader};
use crate::app::parse::parser::HeaderCollection;
use crate::app::sequence::Sequence;
use crate::master::runner::TaskError;
use crate::master::task::TaskStatus;
use crate::master::types::{
    CommandHeader, CommandResponseError, CommandResultHandler, CommandTaskError,
};
use crate::util::cursor::{WriteCursor, WriteError};

enum State {
    Select,
    Operate,
    DirectOperate,
}

pub(crate) struct CommandTask {
    state: State,
    headers: Vec<CommandHeader>,
    handler: Box<dyn CommandResultHandler>,
}

impl CommandTask {
    fn new(
        state: State,
        headers: Vec<CommandHeader>,
        handler: Box<dyn CommandResultHandler>,
    ) -> Self {
        Self {
            state,
            headers,
            handler,
        }
    }

    pub(crate) fn select_before_operate(
        headers: Vec<CommandHeader>,
        handler: Box<dyn CommandResultHandler>,
    ) -> Self {
        Self::new(State::Select, headers, handler)
    }

    pub(crate) fn direct_operate(
        headers: Vec<CommandHeader>,
        handler: Box<dyn CommandResultHandler>,
    ) -> Self {
        Self::new(State::DirectOperate, headers, handler)
    }

    pub(crate) fn format(&self, seq: Sequence, cursor: &mut WriteCursor) -> Result<(), WriteError> {
        let function = match self.state {
            State::DirectOperate => FunctionCode::DirectOperate,
            State::Select => FunctionCode::Select,
            State::Operate => FunctionCode::Operate,
        };

        let mut writer = start_request(Control::request(seq), function, cursor)?;

        for header in self.headers.iter() {
            header.write(&mut writer)?;
        }

        Ok(())
    }

    fn compare(&self, headers: HeaderCollection) -> Result<(), CommandResponseError> {
        let mut iter = headers.iter();

        for sent in &self.headers {
            match iter.next() {
                None => return Err(CommandResponseError::HeaderCountMismatch),
                Some(received) => sent.compare(received.details)?,
            }
        }

        if iter.next().is_some() {
            return Err(CommandResponseError::HeaderCountMismatch);
        }

        Ok(())
    }

    pub(crate) fn handle(
        &mut self,
        _source: u16,
        _response: ResponseHeader,
        headers: HeaderCollection,
    ) -> TaskStatus {
        if let Err(err) = self.compare(headers) {
            self.handler.handle(Err(CommandTaskError::Response(err)));
            return TaskStatus::Complete;
        }

        match self.state {
            State::Select => {
                self.state = State::Operate;
                TaskStatus::ExecuteNextStep
            }
            _ => {
                // Complete w/ success
                self.handler.handle(Ok(()));
                TaskStatus::Complete
            }
        }
    }

    pub(crate) fn on_error(&mut self, error: TaskError) {
        self.handler.handle(Err(CommandTaskError::Task(error)));
    }
}