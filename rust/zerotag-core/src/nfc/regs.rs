// SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD (register map, from M5Unit-NFC)
// SPDX-License-Identifier: MIT
//! ST25R3916 register map, SPI opcodes and bit values.
//! Names and values follow M5Stack's M5Unit-NFC (MIT), src/unit/ST25R3916_definition.hpp.
#![allow(dead_code)]

// Space A registers
pub const REG_IO_CONFIGURATION1: u8 = 0x00;
pub const REG_IO_CONFIGURATION2: u8 = 0x01;
pub const REG_OPERATION_CONTROL: u8 = 0x02;
pub const REG_MODE_DEFINITION: u8 = 0x03;
pub const REG_BITRATE_DEFINITION: u8 = 0x04;
pub const REG_ISO14443A_SETTINGS: u8 = 0x05;
pub const REG_NFCIP1_PASSIVE_TARGET: u8 = 0x08;
pub const REG_AUXILIARY_DEFINITION: u8 = 0x0A;
pub const REG_RECEIVER_CONFIGURATION1: u8 = 0x0B;
pub const REG_RECEIVER_CONFIGURATION2: u8 = 0x0C;
pub const REG_RECEIVER_CONFIGURATION3: u8 = 0x0D;
pub const REG_RECEIVER_CONFIGURATION4: u8 = 0x0E;
pub const REG_MASK_RECEIVER_TIMER: u8 = 0x0F;
pub const REG_NO_RESPONSE_TIMER1: u8 = 0x10;
pub const REG_TIMER_EMV_CONTROL: u8 = 0x12;
pub const REG_MASK_MAIN_INTERRUPT: u8 = 0x16; // 0x16..0x19: main, timer/NFC, error, passive target
pub const REG_MAIN_INTERRUPT: u8 = 0x1A;
pub const REG_ERROR_INTERRUPT: u8 = 0x1C;
pub const REG_PASSIVE_TARGET_INTERRUPT: u8 = 0x1D;
pub const REG_FIFO_STATUS1: u8 = 0x1E;
pub const REG_PASSIVE_TARGET_DISPLAY: u8 = 0x21;
pub const REG_NUMBER_OF_TRANSMITTED_BYTES1: u8 = 0x22;
pub const REG_BITRATE_DETECTION_DISPLAY: u8 = 0x24;
pub const REG_ANTENNA_TUNING_CONTROL1: u8 = 0x26;
pub const REG_ANTENNA_TUNING_CONTROL2: u8 = 0x27;
pub const REG_TX_DRIVER: u8 = 0x28;
pub const REG_PASSIVE_TARGET_MODULATION: u8 = 0x29;
pub const REG_FIELD_DETECTOR_ACTIVATION: u8 = 0x2A;
pub const REG_FIELD_DETECTOR_DEACTIVATION: u8 = 0x2B;
pub const REG_AUXILIARY_DISPLAY: u8 = 0x31;
pub const REG_IC_IDENTITY: u8 = 0x3F;

// Space B registers (accessed after the 0xFB prefix)
pub const REG_EMD_SUPPRESSION: u8 = 0x05;
pub const REG_CORRELATOR_CONFIGURATION1: u8 = 0x0C;
pub const REG_CORRELATOR_CONFIGURATION2: u8 = 0x0D;
pub const REG_RESISTIVE_AM_MODULATION: u8 = 0x2A;
pub const REG_REGULATOR_DISPLAY: u8 = 0x2C;
pub const REG_OVERSHOOT_PROTECTION1: u8 = 0x30;
pub const REG_OVERSHOOT_PROTECTION2: u8 = 0x31;
pub const REG_UNDERSHOOT_PROTECTION1: u8 = 0x32;
pub const REG_UNDERSHOOT_PROTECTION2: u8 = 0x33;

// SPI opcodes
pub const OP_WRITE_REGISTER: u8 = 0x00;
pub const OP_READ_REGISTER: u8 = 0x40;
pub const OP_LOAD_FIFO: u8 = 0x80;
pub const OP_READ_FIFO: u8 = 0x9F;
pub const OP_LOAD_PT_MEMORY_A: u8 = 0xA0;
pub const OP_DIRECT_COMMAND: u8 = 0xC0;
pub const PREFIX_SPACE_B: u8 = 0xFB;
pub const PREFIX_TEST_ACCESS: u8 = 0xFC;

// Direct commands (already include the 0xC0 opcode)
pub const CMD_SET_DEFAULT: u8 = 0xC1;
pub const CMD_STOP_ALL_ACTIVITIES: u8 = 0xC2;
pub const CMD_TRANSMIT_WITH_CRC: u8 = 0xC4;
pub const CMD_TRANSMIT_WITHOUT_CRC: u8 = 0xC5;
pub const CMD_TRANSMIT_REQA: u8 = 0xC6;
pub const CMD_TRANSMIT_WUPA: u8 = 0xC7;
pub const CMD_NFC_INITIAL_FIELD_ON: u8 = 0xC8;
pub const CMD_RESET_RX_GAIN: u8 = 0xD5;
pub const CMD_ADJUST_REGULATORS: u8 = 0xD6;
pub const CMD_CLEAR_FIFO: u8 = 0xDB;
pub const CMD_GO_TO_SENSE: u8 = 0xCD;
pub const CMD_GO_TO_SLEEP: u8 = 0xCE;
pub const CMD_MASK_RECEIVE_DATA: u8 = 0xD0;
pub const CMD_UNMASK_RECEIVE_DATA: u8 = 0xD1;

// Bit values
pub const OP_CONTROL_EN: u8 = 0x80;
pub const OP_CONTROL_RX_EN: u8 = 0x40;
pub const OP_CONTROL_TX_EN: u8 = 0x08;
pub const OP_CONTROL_WU: u8 = 0x04;
pub const SUP_3V: u8 = 0x80;
pub const AAT_EN: u8 = 0x20;
pub const IO_DRV_LVL: u8 = 0x04;
pub const MISO_PD1: u8 = 0x08;
pub const MISO_PD2: u8 = 0x10;
pub const ANTCL: u8 = 0x01;
pub const NO_CRC_RX: u8 = 0x80;
pub const DIS_CORR: u8 = 0x04;
pub const OSC_OK: u8 = 0x10;
pub const EXT_FIELD_DETECTED: u8 = 0x40; // auxiliary display: EFD_O
pub const TARG: u8 = 0x80; // mode definition: target
pub const NFCIP_PT_AUTO_A: u8 = 0x01; // NFCIP-1 passive target: d_106_ac_a (1 = no automatic NFC-A answer)
pub const PTA_STATE_IDLE: u8 = 0x01;
pub const PTA_STATE_HALT: u8 = 0x09;

// Interrupt bits, packed as main<<24 | timer_nfc<<16 | error<<8 | passive
pub const IRQ_OSC: u32 = 0x80 << 24;
pub const IRQ_EON: u32 = 0x10 << 16;
pub const IRQ_EOF: u32 = 0x08 << 16;
pub const IRQ_NFCT: u32 = 0x01 << 16;
pub const IRQ_RXE_PTA: u32 = 0x10;
pub const IRQ_WU_A: u32 = 0x01;
pub const IRQ_WU_AX: u32 = 0x02;
pub const IRQ_RXS: u32 = 0x20 << 24;
pub const IRQ_RXE: u32 = 0x10 << 24;
pub const IRQ_TXE: u32 = 0x08 << 24;
pub const IRQ_COL: u32 = 0x04 << 24;
pub const IRQ_NRE: u32 = 0x40 << 16;
pub const IRQ_CRC: u32 = 0x80 << 8;
pub const IRQ_PAR: u32 = 0x40 << 8;
pub const IRQ_ERR2: u32 = 0x20 << 8;
pub const IRQ_ERR1: u32 = 0x10 << 8;

pub const IDENTITY_TYPE: u8 = 0x05; // bits 7..3 of the identity register
