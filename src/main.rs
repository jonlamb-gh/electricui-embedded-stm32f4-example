#![no_main]
#![no_std]

//use panic_abort as _; // panic handler
use panic_rtt_target as _; // panic handler

mod serial;

#[rtic::app(device = stm32f4xx_hal::pac, dispatchers = [EXTI0, EXTI1, EXTI2])]
mod app {
    use crate::serial::dma::{StaticGrantW, TxTransfer};
    use crate::sys_clock::SystemClock;
    use bbqueue::framed::{FrameConsumer, FrameProducer};
    use bbqueue::{BBBuffer, Consumer, Producer};
    use byteorder::{ByteOrder, LittleEndian};
    use electricui_embedded::prelude::*;
    use err_derive::Error;
    use log::{error, info, warn};
    use rtt_logger::RTTLogger;
    use rtt_target::rtt_init_print;
    use stm32f4xx_hal::{
        dma::{config::DmaConfig, PeripheralToMemory, Stream5, StreamsTuple, Transfer},
        gpio::{Output, Pin, PinState, PushPull},
        pac::{self, DMA1, TIM3, USART2},
        prelude::*,
        serial::config::{Config, StopBits},
        serial::{self, Rx, Serial},
        timer::{counter::CounterUs, Event, MonoTimerUs},
    };

    #[derive(Copy, Clone, Debug, Error)]
    enum Error {
        #[error(display = "EUI packet error. {}", _0)]
        Packet(#[source] electricui_embedded::wire::packet::Error),

        #[error(display = "EUI framing error. {}", _0)]
        Framing(#[source] electricui_embedded::wire::framing::Error),

        #[error(display = "EUI decoder error. {}", _0)]
        Decoder(#[source] electricui_embedded::decoder::Error),

        #[error(display = "BBQ error. {:?}", _0)]
        BBq(#[source] bbqueue::Error),
    }

    /// LED on PC13
    type LedPin = Pin<Output<PushPull>, 'C', 13>;

    const BOARD_ID: u16 = 0xBEEF;
    const BOARD_NAME: MessageId = unsafe { MessageId::new_unchecked(b"my-board") };
    const LED_BLINK: MessageId = unsafe { MessageId::new_unchecked(b"led_blink") };
    const LED_STATE: MessageId = unsafe { MessageId::new_unchecked(b"led_state") };
    const LIT_TIME: MessageId = unsafe { MessageId::new_unchecked(b"lit_time") };

    const EUI_PKT_MAX_SIZE: usize = Packet::<&[u8]>::MAX_PACKET_SIZE;
    const EUI_DECODER_BUFFER_SIZE: usize = Framing::max_encoded_len(EUI_PKT_MAX_SIZE);

    pub const DMA_RX_XFER_SIZE: usize = 32;
    const DMA_RX_Q_LEN: usize = 8;
    const DMA_RX_Q_SIZE: usize = DMA_RX_Q_LEN * DMA_RX_XFER_SIZE;

    const DMA_TX_XFER_SIZE: usize = Framing::max_encoded_len(EUI_PKT_MAX_SIZE);
    const DMA_TX_Q_LEN: usize = 4;
    const DMA_TX_Q_SIZE: usize = DMA_TX_Q_LEN * DMA_TX_XFER_SIZE;

    const LOGGER: RTTLogger = RTTLogger::new(log::LevelFilter::Info);
    static SYS_CLOCK: SystemClock = SystemClock::new();

    #[shared]
    struct Shared {
        #[lock_free]
        rx: Transfer<Stream5<DMA1>, Rx<USART2>, PeripheralToMemory, StaticGrantW<DMA_RX_Q_SIZE>, 4>,
        #[lock_free]
        rx_prod: Producer<'static, DMA_RX_Q_SIZE>,
        #[lock_free]
        tx: TxTransfer<DMA_TX_Q_SIZE>,
        #[lock_free]
        tx_cons: FrameConsumer<'static, DMA_TX_Q_SIZE>,
        #[lock_free]
        tx_prod: FrameProducer<'static, DMA_TX_Q_SIZE>,
        #[lock_free]
        scratch_buf: [u8; EUI_PKT_MAX_SIZE],

        // Example variables
        blink_enable: u8,
        led_state: u8,
        glow_time: u16,
    }

    #[local]
    struct Local {
        led: LedPin,
        timer: CounterUs<TIM3>,
        rx_cons: Consumer<'static, DMA_RX_Q_SIZE>,
        pkt_dec: Decoder<'static, EUI_DECODER_BUFFER_SIZE>,
    }

    #[monotonic(binds = TIM2, default = true)]
    type MicrosecMono = MonoTimerUs<pac::TIM2>;

    #[init(local = [
        rx_q: BBBuffer<DMA_RX_Q_SIZE> = BBBuffer::new(),
        tx_q: BBBuffer<DMA_TX_Q_SIZE> = BBBuffer::new(),
        dec_buf: [u8; EUI_DECODER_BUFFER_SIZE] = [0; EUI_DECODER_BUFFER_SIZE]
    ])]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        rtt_init_print!();
        log::set_logger(&LOGGER)
            .map(|()| log::set_max_level(log::LevelFilter::Info))
            .unwrap();

        info!("Starting");

        // Set up the system clock
        let rcc = ctx.device.RCC.constrain();
        let clocks = rcc
            .cfgr
            .use_hse(25.MHz())
            .sysclk(48.MHz())
            .require_pll48clk()
            .freeze();

        let (mut rx_prod, rx_cons) = ctx.local.rx_q.try_split().unwrap();
        let (tx_prod, tx_cons) = ctx.local.tx_q.try_split_framed().unwrap();

        let gpioa = ctx.device.GPIOA.split();
        //let gpiob = ctx.device.GPIOB.split();
        let gpioc = ctx.device.GPIOC.split();

        let mut led = gpioc.pc13.into_push_pull_output();
        led.set_low();

        // USART2, 115200, 8N1, Rx/Tx DMA
        let tx = gpioa.pa2.into_alternate();
        let rx = gpioa.pa3.into_alternate();
        let mut serial_config = Config::default()
            .baudrate(115_200.bps())
            .wordlength_8()
            .parity_none()
            .stopbits(StopBits::STOP1);
        serial_config.dma = serial::config::DmaConfig::TxRx;
        let serial = Serial::new(ctx.device.USART2, (tx, rx), serial_config, &clocks)
            .unwrap()
            .with_u8_data();
        let (serial_tx, mut serial_rx) = serial.split();
        serial_rx.listen_idle();

        let dma_streams = StreamsTuple::new(ctx.device.DMA1);

        // Setup Rx DMA
        let stream = dma_streams.5;
        let dma_rx_config = DmaConfig::default()
            .transfer_complete_interrupt(true)
            .memory_increment(true);
        let rx_wgrant = rx_prod.grant_exact(DMA_RX_XFER_SIZE).unwrap();
        let mut rx = Transfer::init_peripheral_to_memory(
            stream,
            serial_rx,
            rx_wgrant.into(),
            None,
            dma_rx_config,
        );
        rx.start(|_| ());

        // Setup Tx DMA, gets initialized when started
        let tx = TxTransfer::Uninitialized(dma_streams.6, serial_tx);

        let mut timer = ctx.device.TIM3.counter_us(&clocks);
        timer.start(1.millis()).unwrap();
        timer.listen(Event::Update);

        let mono = ctx.device.TIM2.monotonic_us(&clocks);
        info!("Initialized");

        (
            Shared {
                rx,
                rx_prod,
                tx,
                tx_cons,
                tx_prod,
                scratch_buf: [0; EUI_PKT_MAX_SIZE],
                blink_enable: 1,
                led_state: 0,
                glow_time: 200,
            },
            Local {
                led,
                timer,
                rx_cons,
                pkt_dec: Decoder::new(ctx.local.dec_buf),
            },
            init::Monotonics(mono),
        )
    }

    #[idle(local = [led], shared = [blink_enable, led_state, glow_time])]
    fn idle(ctx: idle::Context) -> ! {
        let led = ctx.local.led;
        let mut blink_enable = ctx.shared.blink_enable;
        let mut led_state = ctx.shared.led_state;
        let mut glow_time = ctx.shared.glow_time;

        // active-low
        let led_state_to_pin_state = |s| match s {
            0 => PinState::High,
            _ => PinState::Low,
        };

        led.set_state(led_state_to_pin_state(led_state.lock(|a| *a)));
        let mut led_timer = SYS_CLOCK.get_raw();

        loop {
            if blink_enable.lock(|a| *a) != 0 {
                let now = SYS_CLOCK.get_raw();
                // NOTE: no overflow checking
                if now - led_timer >= u32::from(glow_time.lock(|a| *a)) {
                    let new_state = led_state.lock(|a| {
                        *a = !*a & 1;
                        *a
                    });
                    led.set_state(led_state_to_pin_state(new_state));
                    led_timer = now;
                }
            }
        }
    }

    fn enqueue_packet<T: AsRef<[u8]>>(
        pkt: &Packet<T>,
        prod: &mut FrameProducer<DMA_TX_Q_SIZE>,
    ) -> Result<(), Error> {
        info!(">> {}", pkt);
        let pkt_len = pkt.wire_size()?;
        let frame_len = Framing::max_encoded_len(pkt_len);
        let mut g = prod.grant(frame_len)?;
        let size = Framing::encode_buf(&pkt.as_ref()[..pkt_len], &mut g);
        g.commit(size);
        start_dma_tx::spawn().ok();
        Ok(())
    }

    #[task(local = [rx_cons, pkt_dec], capacity = 2, priority = 1)]
    fn poll_uart_queue(ctx: poll_uart_queue::Context) {
        let cons = ctx.local.rx_cons;
        let dec = ctx.local.pkt_dec;
        while let Ok(mut grant) = cons.read() {
            let len = grant.len();
            for idx in 0..len {
                grant.to_release(idx + 1);
                match dec.decode(grant.buf()[idx]) {
                    Ok(Some(pkt)) => {
                        if let Err(e) = dispatch_eui_packet(pkt) {
                            warn!("Dispatch error. {}", e);
                        }
                        poll_uart_queue::spawn().ok();
                    }
                    Err(e) => {
                        warn!("Decoder error. {}", e);
                        poll_uart_queue::spawn().ok();
                    }
                    _ => (),
                }
            }
        }
    }

    fn dispatch_eui_packet(pkt: Packet<&[u8]>) -> Result<(), Error> {
        let msg_id = pkt.msg_id()?;
        let is_internal = pkt.internal();
        let resp = pkt.response();
        info!("<< {} Id({})", pkt, msg_id);
        match msg_id {
            MessageId::INTERNAL_HEARTBEAT if is_internal && resp => {
                let hb_num = pkt.payload()?[0];
                eui_packet_handler::spawn(EUiRequest::HeartBeat(hb_num)).ok();
            }
            MessageId::INTERNAL_BOARD_ID if is_internal && resp => {
                eui_packet_handler::spawn(EUiRequest::BoardId).ok();
            }
            MessageId::BOARD_NAME if !is_internal && resp => {
                eui_packet_handler::spawn(EUiRequest::BoardName).ok();
            }
            MessageId::INTERNAL_AM if is_internal => {
                eui_packet_handler::spawn(EUiRequest::AnnounceIds).ok();
            }
            MessageId::INTERNAL_AV if is_internal => {
                eui_packet_handler::spawn(EUiRequest::SendVariables).ok();
            }
            LED_STATE if !is_internal && resp => {
                eui_packet_handler::spawn(EUiRequest::QueryLedState).ok();
            }
            LIT_TIME if !is_internal && resp && (pkt.data_length() == 2) => {
                let new_val = LittleEndian::read_u16(pkt.payload()?);
                eui_packet_handler::spawn(EUiRequest::SetLitTime(new_val)).ok();
            }
            _ => warn!("Message Id({}) ignored", msg_id),
        }
        Ok(())
    }

    #[derive(Debug)]
    pub enum EUiRequest {
        HeartBeat(u8),
        BoardId,
        BoardName,
        AnnounceIds,
        SendVariables,
        QueryLedState,
        SetLitTime(u16),
    }

    #[task(shared = [tx_prod, scratch_buf, blink_enable, led_state, glow_time], priority = 2)]
    fn eui_packet_handler(mut ctx: eui_packet_handler::Context, req: EUiRequest) {
        let prod = ctx.shared.tx_prod;
        let buf = ctx.shared.scratch_buf;
        let res = match req {
            EUiRequest::HeartBeat(val) => send_heartbeat(buf, prod, val),
            EUiRequest::BoardId => send_board_id(buf, prod),
            EUiRequest::BoardName => send_board_name(buf, prod),
            EUiRequest::AnnounceIds => send_id_list(buf, prod),
            EUiRequest::SendVariables => {
                let (blink_enable, led_state, glow_time) = (
                    ctx.shared.blink_enable,
                    ctx.shared.led_state,
                    ctx.shared.glow_time,
                )
                    .lock(|a, b, c| (*a, *b, *c));
                send_tracked_vars(buf, prod, blink_enable, led_state, glow_time)
            }
            EUiRequest::QueryLedState => {
                let led_state = ctx.shared.led_state.lock(|a| *a);
                send_led_state(buf, prod, led_state)
            }
            EUiRequest::SetLitTime(new_val) => {
                let glow_time = ctx.shared.glow_time.lock(|a| {
                    *a = new_val;
                    *a
                });
                send_lit_time(buf, prod, glow_time)
            }
        };
        if let Err(e) = res {
            warn!("Failed to handle request {:?}. {}", req, e);
        }
    }

    fn send_heartbeat(
        buf: &mut [u8],
        prod: &mut FrameProducer<DMA_TX_Q_SIZE>,
        value: u8,
    ) -> Result<(), Error> {
        let mut p = Packet::new_unchecked(&mut buf[..]);
        p.set_data_length(MessageType::U8.data_wire_size(1) as _)?;
        p.set_typ(MessageType::U8);
        p.set_internal(true);
        p.set_offset(false);
        p.set_id_length(MessageId::INTERNAL_HEARTBEAT.len() as _)?;
        p.set_response(false);
        p.set_acknum(0);
        p.msg_id_mut()?
            .copy_from_slice(MessageId::INTERNAL_HEARTBEAT.as_bytes());
        p.payload_mut()?[0] = value;
        p.set_checksum(p.compute_checksum()?)?;
        enqueue_packet(&p, prod)?;
        Ok(())
    }

    fn send_board_id(buf: &mut [u8], prod: &mut FrameProducer<DMA_TX_Q_SIZE>) -> Result<(), Error> {
        let mut p = Packet::new_unchecked(&mut buf[..]);
        p.set_data_length(MessageType::U16.data_wire_size(1) as _)?;
        p.set_typ(MessageType::U16);
        p.set_internal(true);
        p.set_offset(false);
        p.set_id_length(MessageId::INTERNAL_BOARD_ID.len() as _)?;
        p.set_response(false);
        p.set_acknum(0);
        p.msg_id_mut()?
            .copy_from_slice(MessageId::INTERNAL_BOARD_ID.as_bytes());
        LittleEndian::write_u16(p.payload_mut()?, BOARD_ID);
        p.set_checksum(p.compute_checksum()?)?;
        enqueue_packet(&p, prod)?;
        Ok(())
    }

    fn send_board_name(
        buf: &mut [u8],
        prod: &mut FrameProducer<DMA_TX_Q_SIZE>,
    ) -> Result<(), Error> {
        let mut p = Packet::new_unchecked(&mut buf[..]);
        p.set_data_length(BOARD_NAME.len() as _)?;
        p.set_typ(MessageType::Char);
        p.set_internal(false);
        p.set_offset(false);
        p.set_id_length(MessageId::BOARD_NAME.len() as _)?;
        p.set_response(false);
        p.set_acknum(0);
        p.msg_id_mut()?
            .copy_from_slice(MessageId::BOARD_NAME.as_bytes());
        p.payload_mut()?.copy_from_slice(BOARD_NAME.as_bytes());
        p.set_checksum(p.compute_checksum()?)?;
        enqueue_packet(&p, prod)?;
        Ok(())
    }

    fn send_id_list(buf: &mut [u8], prod: &mut FrameProducer<DMA_TX_Q_SIZE>) -> Result<(), Error> {
        // NULL delimited list of ids
        let data_len = LED_BLINK.len() + LED_STATE.len() + LIT_TIME.len() + BOARD_NAME.len() + 4;
        let mut p = Packet::new_unchecked(&mut buf[..]);
        p.set_data_length(data_len as _)?;
        p.set_typ(MessageType::Custom);
        p.set_internal(true);
        p.set_offset(false);
        p.set_id_length(MessageId::INTERNAL_AM_LIST.len() as _)?;
        p.set_response(false);
        p.set_acknum(0);
        p.msg_id_mut()?
            .copy_from_slice(MessageId::INTERNAL_AM_LIST.as_bytes());

        let data = p.payload_mut()?;
        let mut s = 0;
        for id in &[LED_BLINK, LED_STATE, LIT_TIME, BOARD_NAME] {
            let e = s + id.len();
            (&mut data[s..e]).copy_from_slice(id.as_bytes());
            data[e] = b'\0';
            s = e + 1;
        }
        p.set_checksum(p.compute_checksum()?)?;
        enqueue_packet(&p, prod)?;

        // Followed by MessageId::INTERNAL_AM_END, end of ids
        let mut p = Packet::new_unchecked(&mut buf[..]);
        p.set_data_length(1)?;
        p.set_typ(MessageType::U8);
        p.set_internal(true);
        p.set_offset(false);
        p.set_id_length(MessageId::INTERNAL_AM_END.len() as _)?;
        p.set_response(false);
        p.set_acknum(0);
        p.msg_id_mut()?
            .copy_from_slice(MessageId::INTERNAL_AM_END.as_bytes());
        p.payload_mut()?[0] = 4; // 4 tracked msgs / vars
        p.set_checksum(p.compute_checksum()?)?;
        enqueue_packet(&p, prod)?;

        Ok(())
    }

    fn send_tracked_vars(
        buf: &mut [u8],
        prod: &mut FrameProducer<DMA_TX_Q_SIZE>,
        blink_enable: u8,
        led_state: u8,
        glow_time: u16,
    ) -> Result<(), Error> {
        const VARS: [(MessageType, MessageId); 4] = [
            (MessageType::U8, LED_BLINK),
            (MessageType::U8, LED_STATE),
            (MessageType::U16, LIT_TIME),
            (MessageType::Char, BOARD_NAME),
        ];

        for (typ, id) in VARS.iter() {
            let mut p = Packet::new_unchecked(&mut buf[..]);
            if *id == BOARD_NAME {
                p.set_data_length(typ.data_wire_size(BOARD_NAME.len()) as _)?;
            } else {
                p.set_data_length(typ.data_wire_size(1) as _)?;
            }
            p.set_typ(*typ);
            p.set_internal(false);
            p.set_offset(false);
            p.set_id_length(id.len() as _)?;
            p.set_response(false);
            p.set_acknum(0);
            p.msg_id_mut()?.copy_from_slice(id.as_bytes());
            let data = p.payload_mut()?;
            match *id {
                LED_BLINK => data[0] = blink_enable,
                LED_STATE => data[0] = led_state,
                LIT_TIME => LittleEndian::write_u16(data, glow_time),
                BOARD_NAME => data.copy_from_slice(BOARD_NAME.as_bytes()),
                _ => (),
            }
            p.set_checksum(p.compute_checksum()?)?;
            enqueue_packet(&p, prod)?;
        }

        Ok(())
    }

    fn send_led_state(
        buf: &mut [u8],
        prod: &mut FrameProducer<DMA_TX_Q_SIZE>,
        led_state: u8,
    ) -> Result<(), Error> {
        let mut p = Packet::new_unchecked(&mut buf[..]);
        p.set_data_length(1)?;
        p.set_typ(MessageType::U8);
        p.set_internal(false);
        p.set_offset(false);
        p.set_id_length(LED_STATE.len() as _)?;
        p.set_response(false);
        p.set_acknum(0);
        p.msg_id_mut()?.copy_from_slice(LED_STATE.as_bytes());
        p.payload_mut()?[0] = led_state;
        p.set_checksum(p.compute_checksum()?)?;
        enqueue_packet(&p, prod)?;
        Ok(())
    }

    fn send_lit_time(
        buf: &mut [u8],
        prod: &mut FrameProducer<DMA_TX_Q_SIZE>,
        glow_time: u16,
    ) -> Result<(), Error> {
        let mut p = Packet::new_unchecked(&mut buf[..]);
        p.set_data_length(2)?;
        p.set_typ(MessageType::U16);
        p.set_internal(false);
        p.set_offset(false);
        p.set_id_length(LIT_TIME.len() as _)?;
        p.set_response(false);
        p.set_acknum(0);
        p.msg_id_mut()?.copy_from_slice(LIT_TIME.as_bytes());
        LittleEndian::write_u16(p.payload_mut()?, glow_time);
        p.set_checksum(p.compute_checksum()?)?;
        enqueue_packet(&p, prod)?;
        Ok(())
    }

    use crate::serial::on_dma_rx_complete;
    extern "Rust" {
        #[task(binds=DMA1_STREAM5, shared = [rx, rx_prod], priority = 3)]
        fn on_dma_rx_complete(ctx: on_dma_rx_complete::Context);
    }

    use crate::serial::on_idle;
    extern "Rust" {
        #[task(binds = USART2, shared = [rx, rx_prod], priority = 3)]
        fn on_idle(ctx: on_idle::Context);
    }

    use crate::serial::start_dma_tx;
    extern "Rust" {
        #[task(shared = [tx, tx_cons], capacity = 2, priority = 2)]
        fn start_dma_tx(ctx: start_dma_tx::Context);
    }

    use crate::serial::on_dma_tx_complete;
    extern "Rust" {
        #[task(binds=DMA1_STREAM6, shared = [tx], priority = 2)]
        fn on_dma_tx_complete(ctx: on_dma_tx_complete::Context);
    }

    #[task(binds=TIM3, local = [timer], priority = 4)]
    fn on_timer(ctx: on_timer::Context) {
        let timer = ctx.local.timer;
        let _ = timer.wait();
        SYS_CLOCK.inc_from_interrupt();
    }
}

mod sys_clock {
    use core::sync::atomic::{AtomicU32, Ordering::SeqCst};

    /// 32-bit millisecond clock
    #[derive(Debug)]
    #[repr(transparent)]
    pub struct SystemClock(AtomicU32);

    impl SystemClock {
        pub const fn new() -> Self {
            SystemClock(AtomicU32::new(0))
        }

        pub fn inc_from_interrupt(&self) {
            self.0.fetch_add(1, SeqCst);
        }

        pub fn get_raw(&self) -> u32 {
            self.0.load(SeqCst)
        }
    }
}
