/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD (register map, from M5Unit-NFC)
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include <cstdint>

// ST25R3916 register map, SPI opcodes and bit values.
// Names and values follow M5Stack's M5Unit-NFC (MIT), src/unit/ST25R3916_definition.hpp.
namespace platform::st25r {

// Space A registers
constexpr uint8_t kRegIoConfiguration1 = 0x00;
constexpr uint8_t kRegIoConfiguration2 = 0x01;
constexpr uint8_t kRegOperationControl = 0x02;
constexpr uint8_t kRegModeDefinition = 0x03;
constexpr uint8_t kRegBitrateDefinition = 0x04;
constexpr uint8_t kRegIso14443aSettings = 0x05;
constexpr uint8_t kRegNfcip1PassiveTarget = 0x08;
constexpr uint8_t kRegAuxiliaryDefinition = 0x0A;
constexpr uint8_t kRegReceiverConfiguration1 = 0x0B;
constexpr uint8_t kRegReceiverConfiguration2 = 0x0C;
constexpr uint8_t kRegReceiverConfiguration3 = 0x0D;
constexpr uint8_t kRegReceiverConfiguration4 = 0x0E;
constexpr uint8_t kRegMaskReceiverTimer = 0x0F;
constexpr uint8_t kRegNoResponseTimer1 = 0x10;
constexpr uint8_t kRegTimerEmvControl = 0x12;
constexpr uint8_t kRegMaskMainInterrupt = 0x16; // 0x16..0x19: main, timer/NFC, error, passive target
constexpr uint8_t kRegMainInterrupt = 0x1A;
constexpr uint8_t kRegErrorInterrupt = 0x1C;
constexpr uint8_t kRegPassiveTargetInterrupt = 0x1D;
constexpr uint8_t kRegFifoStatus1 = 0x1E;
constexpr uint8_t kRegPassiveTargetDisplay = 0x21;
constexpr uint8_t kRegNumberOfTransmittedBytes1 = 0x22;
constexpr uint8_t kRegBitrateDetectionDisplay = 0x24;
constexpr uint8_t kRegAntennaTuningControl1 = 0x26;
constexpr uint8_t kRegAntennaTuningControl2 = 0x27;
constexpr uint8_t kRegTxDriver = 0x28;
constexpr uint8_t kRegPassiveTargetModulation = 0x29;
constexpr uint8_t kRegFieldDetectorActivation = 0x2A;
constexpr uint8_t kRegFieldDetectorDeactivation = 0x2B;
constexpr uint8_t kRegAuxiliaryDisplay = 0x31;
constexpr uint8_t kRegIcIdentity = 0x3F;

// Space B registers (accessed after the 0xFB prefix)
constexpr uint8_t kRegEmdSuppression = 0x05;
constexpr uint8_t kRegCorrelatorConfiguration1 = 0x0C;
constexpr uint8_t kRegCorrelatorConfiguration2 = 0x0D;
constexpr uint8_t kRegResistiveAmModulation = 0x2A;
constexpr uint8_t kRegRegulatorDisplay = 0x2C;
constexpr uint8_t kRegOvershootProtection1 = 0x30;
constexpr uint8_t kRegOvershootProtection2 = 0x31;
constexpr uint8_t kRegUndershootProtection1 = 0x32;
constexpr uint8_t kRegUndershootProtection2 = 0x33;

// SPI opcodes
constexpr uint8_t kOpWriteRegister = 0x00;
constexpr uint8_t kOpReadRegister = 0x40;
constexpr uint8_t kOpLoadFifo = 0x80;
constexpr uint8_t kOpReadFifo = 0x9F;
constexpr uint8_t kOpLoadPtMemoryA = 0xA0;
constexpr uint8_t kOpDirectCommand = 0xC0;
constexpr uint8_t kPrefixSpaceB = 0xFB;
constexpr uint8_t kPrefixTestAccess = 0xFC;

// Direct commands (already include the 0xC0 opcode)
constexpr uint8_t kCmdSetDefault = 0xC1;
constexpr uint8_t kCmdStopAllActivities = 0xC2;
constexpr uint8_t kCmdTransmitWithCrc = 0xC4;
constexpr uint8_t kCmdTransmitWithoutCrc = 0xC5;
constexpr uint8_t kCmdTransmitReqa = 0xC6;
constexpr uint8_t kCmdTransmitWupa = 0xC7;
constexpr uint8_t kCmdNfcInitialFieldOn = 0xC8;
constexpr uint8_t kCmdResetRxGain = 0xD5;
constexpr uint8_t kCmdAdjustRegulators = 0xD6;
constexpr uint8_t kCmdClearFifo = 0xDB;
constexpr uint8_t kCmdGoToSense = 0xCD;
constexpr uint8_t kCmdGoToSleep = 0xCE;
constexpr uint8_t kCmdMaskReceiveData = 0xD0;
constexpr uint8_t kCmdUnmaskReceiveData = 0xD1;

// Bit values
constexpr uint8_t kOpControlEn = 0x80;
constexpr uint8_t kOpControlRxEn = 0x40;
constexpr uint8_t kOpControlTxEn = 0x08;
constexpr uint8_t kOpControlWu = 0x04;
constexpr uint8_t kSup3v = 0x80;
constexpr uint8_t kAatEn = 0x20;
constexpr uint8_t kIoDrvLvl = 0x04;
constexpr uint8_t kMisoPd1 = 0x08;
constexpr uint8_t kMisoPd2 = 0x10;
constexpr uint8_t kAntcl = 0x01;
constexpr uint8_t kNoCrcRx = 0x80;
constexpr uint8_t kDisCorr = 0x04;
constexpr uint8_t kOscOk = 0x10;
constexpr uint8_t kExtFieldDetected = 0x40; // auxiliary display: EFD_O
constexpr uint8_t kTarg = 0x80;             // mode definition: target
constexpr uint8_t kNfcipPtAutoA = 0x01;     // NFCIP-1 passive target: d_106_ac_a (1 = no automatic NFC-A answer)
constexpr uint8_t kPtaStateIdle = 0x01;
constexpr uint8_t kPtaStateHalt = 0x09;

// Interrupt bits, packed as main<<24 | timer_nfc<<16 | error<<8 | passive
constexpr uint32_t kIrqOsc = 0x80u << 24;
constexpr uint32_t kIrqEon = 0x10u << 16;
constexpr uint32_t kIrqEof = 0x08u << 16;
constexpr uint32_t kIrqNfct = 0x01u << 16;
constexpr uint32_t kIrqRxePta = 0x10u;
constexpr uint32_t kIrqWuA = 0x01u;
constexpr uint32_t kIrqWuAx = 0x02u;
constexpr uint32_t kIrqRxs = 0x20u << 24;
constexpr uint32_t kIrqRxe = 0x10u << 24;
constexpr uint32_t kIrqTxe = 0x08u << 24;
constexpr uint32_t kIrqCol = 0x04u << 24;
constexpr uint32_t kIrqNre = 0x40u << 16;
constexpr uint32_t kIrqCrc = 0x80u << 8;
constexpr uint32_t kIrqPar = 0x40u << 8;
constexpr uint32_t kIrqErr2 = 0x20u << 8;
constexpr uint32_t kIrqErr1 = 0x10u << 8;

constexpr uint8_t kIdentityType = 0x05; // bits 7..3 of the identity register

} // namespace platform::st25r
