use super::*;
use lorawan::certification::DutResetReqCreator;
use lorawan::creator::DataPayloadCreator;

fn handle_dut_reset_req(_uplink: Option<Uplink>, _config: RfConfig, rx_buffer: &mut [u8]) -> usize {
    let req = DutResetReqCreator::new();
    let req = req.build();

    // Create a downlink frame containing the DutResetReq
    let mut phy = DataPayloadCreator::new(rx_buffer).unwrap();
    phy.set_f_port(224);
    phy.set_dev_addr(&[0; 4]);
    phy.set_uplink(false);
    phy.set_fcnt(0);

    let finished =
        phy.build(req, &[], &get_key().into(), &get_key().into(), &DefaultFactory).unwrap();
    finished.len()
}

#[cfg(feature = "class-c")]
#[tokio::test]
async fn test_certification_class_c_response() {
    let (radio, _timer, mut async_device) = util::setup_with_session_class_c().await;

    // Run the device listening for the setup message
    let task = tokio::spawn(async move {
        let response = async_device.rxc_listen().await;
        (async_device, response)
    });

    radio.handle_rxtx(handle_dut_reset_req).await;
    let (device, _response) = task.await.unwrap();

    assert_eq!(device.device.dut_reset_called, true);
}
