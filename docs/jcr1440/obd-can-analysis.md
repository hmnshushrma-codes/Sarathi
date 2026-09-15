# OBD-II / CAN Architecture Analysis

## Architecture Overview

The JCR1440 uses a **dual-processor architecture** for vehicle communication:

```
OBD-II Port (16-pin)
    │
    ├── Pin 6  — CAN High
    ├── Pin 14 — CAN Low
    ├── Pin 16 — +12V Battery
    ├── Pin 4  — Chassis Ground
    └── Pin 5  — Signal Ground
         │
         ▼
┌─────────────────────┐
│   CAN Transceiver   │
│   (likely TJA1050   │
│    or MCP2551)      │
└─────────┬───────────┘
          │ CAN RX/TX
          ▼
┌─────────────────────┐
│   NXP IMXRT1051     │
│   (Cortex-M7 MCU)   │
│                     │
│   FlexCAN module    │
│   512KB RAM         │
│   1MB SPI Flash     │
│   VCI FW v4.0.0     │
└─────────┬───────────┘
          │ Internal bus (UART/SPI)
          ▼
┌─────────────────────┐
│   Qualcomm MDM9x07  │
│   (Main SoC)        │
│                     │
│   Linux application │
│   Web API           │
│   Cloud telemetry   │
└─────────────────────┘
```

## OBD Protocol Support

From the official manual:

| Protocol          | Standard         | Support           |
| ----------------- | ---------------- | ----------------- |
| CAN ISO 15765-4   | 11-bit CAN ID    | YES               |
| CAN ISO 15765-4   | 29-bit CAN ID    | YES               |
| CAN 250 kbaud     |                  | YES               |
| CAN 500 kbaud     |                  | YES               |
| ISO 9141-2        | K-line            | Not listed        |
| KWP2000           | ISO 14230         | Not listed        |
| J1850 VPW         | GM protocol       | Not listed        |
| J1850 PWM         | Ford protocol     | Not listed        |

The device supports **CAN-only OBD-II** (ISO 15765-4), which covers
most vehicles manufactured after 2008. Older vehicles using K-line or
J1850 protocols are NOT supported.

## NXP IMXRT1051 — Vehicle MCU Details

The NXP i.MX RT1051 (IMXRT1051CVL5) is a high-performance MCU:

- **Core**: ARM Cortex-M7 @ 600 MHz
- **FlexCAN**: Hardware CAN 2.0B controller
  - Supports 11-bit (standard) and 29-bit (extended) CAN IDs
  - Up to 1 Mbps
  - 64 message buffers
  - Hardware filtering
- **Memory**: 512 KB SRAM + 1 MB external SPI Flash
- **Interfaces**: UART, SPI, I2C, GPIO, ADC, PWM

This is a significantly more powerful MCU than a typical ELM327 chip.
It can perform complex CAN operations including:
- Multi-frame ISO-TP messages
- Raw CAN frame capture
- DTC (Diagnostic Trouble Code) reading
- PID polling
- UDS (Unified Diagnostic Services)

## Web API — CAN/OBD Endpoints

Two endpoints were discovered:

| Endpoint         | Purpose                    | Auth Required |
| ---------------- | -------------------------- | ------------- |
| `/st_can.w.xml`  | CAN bus status/data        | YES           |
| `/st_obd.w.xml`  | OBD-II diagnostic data     | YES           |

Both redirect to `login.htm` without a valid session.

## VCI (Vehicle Communication Interface)

The device reports a VCI version (4.0.0), suggesting the NXP MCU runs
a dedicated VCI firmware separate from the main Linux system.

From the device status XML:
```xml
<vciver>4.0.0</vciver>
```

This VCI firmware likely:
1. Initializes the CAN controller
2. Performs OBD-II protocol negotiation
3. Polls standard PIDs (RPM, speed, coolant temp, etc.)
4. Reads DTCs (Diagnostic Trouble Codes)
5. Forwards data to the main SoC over internal bus
6. May support raw CAN frame capture

## Vehicle Health Monitoring

The JioThings app shows the device can monitor:

- Engine RPM
- Vehicle speed
- Coolant temperature
- Air intake temperature
- Battery voltage
- Diagnostic Trouble Codes (DTCs)
- Driving behavior (harsh braking, acceleration, turning)

This data is collected by the NXP MCU and transmitted via the Qualcomm
SoC to Jio's cloud platform.

## CAN Bus Direct Access — Current Status

The USB interface does **NOT** expose a direct CAN interface. There is:
- No SocketCAN interface (can0)
- No ELM327-compatible serial port
- No USB-CAN adapter emulation
- No raw CAN frame access

CAN/OBD data appears to be accessible **only** through:
1. The web API (after authentication)
2. The JioThings cloud platform

## Implications for Repurposing

### What works now
- OBD-II data is readable via the web API (needs login)
- The device handles all CAN protocol complexity internally
- Standard PIDs and DTCs are supported

### What would require modification
- Direct CAN bus access (raw frames)
- Custom PID definitions
- Extended diagnostics (UDS services beyond OBD-II)
- Passive CAN logging
- CAN frame injection (DO NOT DO THIS — safety risk)

### Comparison with standalone OBD tools

| Feature            | JCR1440 (via API)    | ELM327 USB     | SocketCAN |
| ------------------ | -------------------- | -------------- | --------- |
| Standard PIDs      | Yes (via web API)    | Yes            | Yes       |
| DTCs               | Yes (via web API)    | Yes            | Yes       |
| Raw CAN frames     | Unknown/unlikely     | Limited        | Yes       |
| Custom PIDs        | Unknown              | Yes            | Yes       |
| CAN logging        | Unknown              | Limited        | Yes       |
| Protocol support   | CAN only             | All OBD-II     | CAN only  |
| Ease of access     | HTTP API             | Serial/AT      | SocketCAN |
| Additional HW      | None needed          | None           | USB-CAN   |

## Safety Warning

**DO NOT** send arbitrary CAN frames to the vehicle bus. The OBD-II
connector shares the CAN bus with critical vehicle systems (ABS, airbags,
engine management). Uncontrolled CAN writes could cause:

- Unintended acceleration/braking
- Airbag deployment
- Engine shutdown
- Instrument cluster malfunction
- ECU damage

Read-only observation is safe. Write operations should only be performed
with full understanding of the vehicle's CAN database and safety implications.
