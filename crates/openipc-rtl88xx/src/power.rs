use crate::regs::*;

#[derive(Debug, Clone, Copy)]
pub(crate) enum PowerCommand {
    Write,
    Polling,
    DelayMs,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PowerStep {
    pub(crate) offset: u16,
    pub(crate) cmd: PowerCommand,
    pub(crate) mask: u8,
    pub(crate) value: u8,
}

pub(crate) const RTL8812_POWER_ON_FLOW: &[PowerStep] = &[
    PowerStep {
        offset: 0x0012,
        cmd: PowerCommand::Write,
        mask: BIT0 as u8,
        value: BIT0 as u8,
    },
    PowerStep {
        offset: 0x0014,
        cmd: PowerCommand::Write,
        mask: 0x80,
        value: 0,
    },
    PowerStep {
        offset: 0x0015,
        cmd: PowerCommand::Write,
        mask: 0x01,
        value: 0,
    },
    PowerStep {
        offset: 0x0023,
        cmd: PowerCommand::Write,
        mask: 0x10,
        value: 0,
    },
    PowerStep {
        offset: 0x0046,
        cmd: PowerCommand::Write,
        mask: 0xff,
        value: 0,
    },
    PowerStep {
        offset: 0x0043,
        cmd: PowerCommand::Write,
        mask: 0xff,
        value: 0,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: BIT2 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: BIT3 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0003,
        cmd: PowerCommand::Write,
        mask: BIT2 as u8,
        value: BIT2 as u8,
    },
    PowerStep {
        offset: 0x0301,
        cmd: PowerCommand::Write,
        mask: 0xff,
        value: 0,
    },
    PowerStep {
        offset: 0x0024,
        cmd: PowerCommand::Write,
        mask: BIT1 as u8,
        value: BIT1 as u8,
    },
    PowerStep {
        offset: 0x0028,
        cmd: PowerCommand::Write,
        mask: BIT3 as u8,
        value: BIT3 as u8,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: BIT2 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0006,
        cmd: PowerCommand::Polling,
        mask: BIT1 as u8,
        value: BIT1 as u8,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: BIT3 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: BIT0 as u8,
        value: BIT0 as u8,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Polling,
        mask: BIT0 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0024,
        cmd: PowerCommand::Write,
        mask: BIT1 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0028,
        cmd: PowerCommand::Write,
        mask: BIT3 as u8,
        value: 0,
    },
];

pub(crate) const RTL8821_POWER_ON_FLOW: &[PowerStep] = &[
    PowerStep {
        offset: 0x0020,
        cmd: PowerCommand::Write,
        mask: BIT0 as u8,
        value: BIT0 as u8,
    },
    PowerStep {
        offset: 0x0067,
        cmd: PowerCommand::Write,
        mask: BIT4 as u8,
        value: 0,
    },
    PowerStep {
        offset: 1,
        cmd: PowerCommand::DelayMs,
        mask: 0,
        value: 0,
    },
    PowerStep {
        offset: 0x0000,
        cmd: PowerCommand::Write,
        mask: BIT5 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: (BIT4 | BIT3 | BIT2) as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0006,
        cmd: PowerCommand::Polling,
        mask: BIT1 as u8,
        value: BIT1 as u8,
    },
    PowerStep {
        offset: 0x0006,
        cmd: PowerCommand::Write,
        mask: BIT0 as u8,
        value: BIT0 as u8,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: BIT7 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: (BIT4 | BIT3) as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Write,
        mask: BIT0 as u8,
        value: BIT0 as u8,
    },
    PowerStep {
        offset: 0x0005,
        cmd: PowerCommand::Polling,
        mask: BIT0 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x004f,
        cmd: PowerCommand::Write,
        mask: BIT0 as u8,
        value: BIT0 as u8,
    },
    PowerStep {
        offset: 0x0067,
        cmd: PowerCommand::Write,
        mask: (BIT5 | BIT4) as u8,
        value: (BIT5 | BIT4) as u8,
    },
    PowerStep {
        offset: 0x0025,
        cmd: PowerCommand::Write,
        mask: BIT6 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0049,
        cmd: PowerCommand::Write,
        mask: BIT1 as u8,
        value: BIT1 as u8,
    },
    PowerStep {
        offset: 0x0063,
        cmd: PowerCommand::Write,
        mask: BIT1 as u8,
        value: BIT1 as u8,
    },
    PowerStep {
        offset: 0x0062,
        cmd: PowerCommand::Write,
        mask: BIT1 as u8,
        value: 0,
    },
    PowerStep {
        offset: 0x0058,
        cmd: PowerCommand::Write,
        mask: BIT0 as u8,
        value: BIT0 as u8,
    },
    PowerStep {
        offset: 0x005a,
        cmd: PowerCommand::Write,
        mask: BIT1 as u8,
        value: BIT1 as u8,
    },
    PowerStep {
        offset: 0x002e,
        cmd: PowerCommand::Write,
        mask: 0xff,
        value: 0x82,
    },
];
