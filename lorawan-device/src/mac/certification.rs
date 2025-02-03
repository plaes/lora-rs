use crate::mac;
use lorawan::certification::{parse_downlink_certification_messages, DownlinkDUTCommand};

#[derive(Debug)]
#[cfg_attr(feature = "defmt-03", derive(defmt::Format))]
pub enum Response {
    NoUpdate,
    DutReset,
}

pub struct Certification {}

impl Certification {
    pub(crate) fn handle_message(&mut self, data: &[u8]) -> Response {
        let messages = parse_downlink_certification_messages(data);
        for message in messages {
            match message {
                DownlinkDUTCommand::DutResetReq(_) => return Response::DutReset,
            }
        }
        /* TODO: Report.. ? */
        Response::NoUpdate
    }
}

impl From<Response> for mac::Response {
    fn from(m: Response) -> Self {
        mac::Response::Certification(m)
    }
}
