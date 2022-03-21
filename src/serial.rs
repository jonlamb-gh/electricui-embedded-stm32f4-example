use crate::app::{
    on_dma_rx_complete, on_dma_tx_complete, on_idle, poll_uart_queue, start_dma_tx,
    DMA_RX_XFER_SIZE,
};
use core::mem;
use dma::TxTransfer;
use stm32f4xx_hal::{
    dma::{
        config::DmaConfig,
        traits::{Stream, StreamISR},
        StreamX, Transfer,
    },
    pac::DMA1,
};

pub(crate) fn on_dma_rx_complete(ctx: on_dma_rx_complete::Context) {
    let rx = ctx.shared.rx;
    let prod = ctx.shared.rx_prod;
    // TODO - get/clear FIFO errors
    let _ = unsafe {
        rx.next_transfer_with(|g, _| {
            g.0.commit(DMA_RX_XFER_SIZE);
            let next_g = prod.grant_exact(DMA_RX_XFER_SIZE).unwrap();
            (next_g.into(), ())
        })
        .unwrap()
    };

    poll_uart_queue::spawn().ok();
}

pub(crate) fn on_idle(ctx: on_idle::Context) {
    let rx = ctx.shared.rx;
    let prod = ctx.shared.rx_prod;
    rx.pause(|serial| serial.clear_idle_interrupt());
    let cnt = DMA_RX_XFER_SIZE - StreamX::<DMA1, 5>::get_number_of_transfers() as usize;
    unsafe {
        let _ = rx.next_transfer_with(|g, _| {
            g.0.commit(cnt);
            let next_g = prod.grant_exact(DMA_RX_XFER_SIZE).unwrap();
            (next_g.into(), ())
        });
    };

    poll_uart_queue::spawn().ok();
}

pub(crate) fn start_dma_tx(ctx: start_dma_tx::Context) {
    let txfer = ctx.shared.tx;
    let cons = ctx.shared.tx_cons;

    // Do nothing if no data to transfer
    let next_g = match cons.read() {
        Some(g) => g,
        _ => return,
    };

    // TODO - clean this up, inefficient to stop/start,
    // should be able to just reload
    if txfer.is_uninitialized() {
        if let TxTransfer::Uninitialized(stream, serial_tx) = mem::replace(txfer, TxTransfer::Empty)
        {
            let dma_config = DmaConfig::default()
                .transfer_complete_interrupt(true)
                .memory_increment(true);
            let mut tx = Transfer::init_memory_to_peripheral(
                stream,
                serial_tx,
                next_g.into(),
                None,
                dma_config,
            );
            tx.start(|_| ());
            let _ = mem::replace(txfer, TxTransfer::Initialized(tx));
        }
    }
}

pub(crate) fn on_dma_tx_complete(ctx: on_dma_tx_complete::Context) {
    let txfer = ctx.shared.tx;

    // TODO
    // - clean this up, inefficient to stop/start, should be able to just reload
    // - get/clear FIFO errors
    if txfer.is_initialized() && StreamX::<DMA1, 6>::get_transfer_complete_flag() {
        if let TxTransfer::Initialized(mut tx) = mem::replace(txfer, TxTransfer::Empty) {
            tx.clear_transfer_complete_interrupt();
            let (stream, serial_tx, g, _) = tx.release();
            g.0.release();
            let _ = mem::replace(txfer, TxTransfer::Uninitialized(stream, serial_tx));
            start_dma_tx::spawn().ok();
        }
    }
}

pub(crate) mod dma {
    use bbqueue::framed::{FrameGrantR, FrameGrantW};
    use bbqueue::{GrantR, GrantW};
    use core::ops::{Deref, DerefMut};
    use embedded_dma::{ReadBuffer, WriteBuffer};
    use stm32f4xx_hal::{
        dma::{MemoryToPeripheral, Stream6, Transfer},
        pac::{DMA1, USART2},
        serial::Tx,
    };

    pub struct StaticGrantW<const N: usize>(pub GrantW<'static, N>);

    impl<const N: usize> From<GrantW<'static, N>> for StaticGrantW<N> {
        fn from(g: GrantW<'static, N>) -> Self {
            Self(g)
        }
    }

    unsafe impl<const N: usize> WriteBuffer for StaticGrantW<N> {
        type Word = u8;

        unsafe fn write_buffer(&mut self) -> (*mut Self::Word, usize) {
            let buf = self.0.buf();
            (buf.as_mut_ptr(), buf.len())
        }
    }

    pub struct StaticGrantR<const N: usize>(pub GrantR<'static, N>);

    impl<const N: usize> From<GrantR<'static, N>> for StaticGrantR<N> {
        fn from(g: GrantR<'static, N>) -> Self {
            Self(g)
        }
    }

    unsafe impl<const N: usize> ReadBuffer for StaticGrantR<N> {
        type Word = u8;

        unsafe fn read_buffer(&self) -> (*const Self::Word, usize) {
            let buf = self.0.buf();
            (buf.as_ptr(), buf.len())
        }
    }

    pub struct StaticFrameGrantW<const N: usize>(pub FrameGrantW<'static, N>);

    impl<const N: usize> From<FrameGrantW<'static, N>> for StaticFrameGrantW<N> {
        fn from(g: FrameGrantW<'static, N>) -> Self {
            Self(g)
        }
    }

    unsafe impl<const N: usize> WriteBuffer for StaticFrameGrantW<N> {
        type Word = u8;

        unsafe fn write_buffer(&mut self) -> (*mut Self::Word, usize) {
            let buf = self.0.deref_mut();
            let len = buf.len();
            (buf.as_mut_ptr(), len)
        }
    }

    pub struct StaticFrameGrantR<const N: usize>(pub FrameGrantR<'static, N>);

    impl<const N: usize> From<FrameGrantR<'static, N>> for StaticFrameGrantR<N> {
        fn from(g: FrameGrantR<'static, N>) -> Self {
            Self(g)
        }
    }

    unsafe impl<const N: usize> ReadBuffer for StaticFrameGrantR<N> {
        type Word = u8;

        unsafe fn read_buffer(&self) -> (*const Self::Word, usize) {
            let buf = self.0.deref();
            let len = buf.len();
            (buf.as_ptr(), len)
        }
    }

    pub enum TxTransfer<const N: usize> {
        Empty,
        Uninitialized(Stream6<DMA1>, Tx<USART2>),
        Initialized(
            Transfer<Stream6<DMA1>, Tx<USART2>, MemoryToPeripheral, StaticFrameGrantR<N>, 4>,
        ),
    }

    impl<const N: usize> TxTransfer<N> {
        pub fn is_initialized(&self) -> bool {
            matches!(self, TxTransfer::Initialized(_))
        }

        pub fn is_uninitialized(&self) -> bool {
            matches!(self, TxTransfer::Uninitialized(_, _))
        }
    }
}
