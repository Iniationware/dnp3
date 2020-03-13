use crate::error::Error;
use crate::link::formatter::LinkFormatter;
use crate::link::function::Function;
use crate::link::header::{Address, ControlField};
use crate::link::parser::FramePayload;
use crate::util::cursor::WriteCursor;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

enum SecondaryState {
    NotReset,
    Reset(bool), // the next expect fcb
}

pub struct Layer {
    secondary_state: SecondaryState,
    formatter: LinkFormatter,
    reader: super::reader::Reader,
    reply_buffer: [u8; super::constant::MAX_LINK_FRAME_LENGTH],
}

impl Layer {
    pub fn new(is_master: bool, address: u16) -> Self {
        Self {
            secondary_state: SecondaryState::NotReset,
            formatter: LinkFormatter::new(is_master, address),
            reader: super::reader::Reader::default(),
            reply_buffer: [0; super::constant::MAX_LINK_FRAME_LENGTH],
        }
    }

    pub fn reset(&mut self) {
        self.secondary_state = SecondaryState::NotReset;
        self.reader.reset();
    }

    async fn reply<T>(
        &mut self,
        destination: u16,
        control: ControlField,
        io: &mut T,
    ) -> Result<(), Error>
    where
        T: AsyncWrite + Unpin,
    {
        let mut cursor = WriteCursor::new(self.reply_buffer.as_mut());
        let start = cursor.position();
        self.formatter
            .format_header_only(destination, control, &mut cursor)?;
        let reply_frame = cursor.written_since(start)?;
        Ok(io.write_all(reply_frame).await?)
    }

    async fn acknowledge<T>(&mut self, destination: u16, io: &mut T) -> Result<(), Error>
    where
        T: AsyncWrite + Unpin,
    {
        self.reply(
            destination,
            ControlField::new(self.formatter.is_master(), Function::SecAck),
            io,
        )
        .await
    }

    pub async fn read<T>(
        &mut self,
        io: &mut T,
        payload: &mut FramePayload,
    ) -> Result<Address, Error>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        loop {
            if let Some(address) = self.read_one(io, payload).await? {
                return Ok(address);
            }
        }
    }

    pub async fn read_one<T>(
        &mut self,
        io: &mut T,
        payload: &mut FramePayload,
    ) -> Result<Option<Address>, Error>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let header = self.reader.read(io, payload).await?;

        if header.control.master == self.formatter.is_master() {
            // don't process frames if this bit is the same
            // masters can't talk to masters, etc
            // TODO - logging
            return Ok(None);
        }

        // TODO - handle broadcast
        if header.address.destination == self.formatter.get_address() {
            // don't process frames for the wrong address
            // TODO - logging
            return Ok(None);
        }

        match header.control.func {
            Function::PriUnconfirmedUserData => Ok(Some(header.address)),
            Function::PriResetLinkStates => {
                self.secondary_state = SecondaryState::Reset(true); // TODO - does it start true or false?
                self.acknowledge(header.address.source, io).await?;
                Ok(None)
            }
            Function::PriConfirmedUserData => {
                match self.secondary_state {
                    SecondaryState::NotReset => {
                        // TODO log and ignore
                        Ok(None)
                    }
                    SecondaryState::Reset(expected) => {
                        if header.control.fcb == expected {
                            self.secondary_state = SecondaryState::Reset(!expected);
                            Ok(Some(header.address))
                        } else {
                            // TODO log and ignore
                            Ok(None)
                        }
                    }
                }
            }
            _ => Ok(None),
        }
    }
}