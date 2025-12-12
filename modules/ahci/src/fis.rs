//! FIS (Frame Information Structure) definitions

/// FIS types
pub const FIS_TYPE_REG_H2D: u8 = 0x27;  // Host to Device
pub const FIS_TYPE_REG_D2H: u8 = 0x34;  // Device to Host
pub const FIS_TYPE_DMA_SETUP: u8 = 0x41;
pub const FIS_TYPE_DATA: u8 = 0x46;

/// Host to Device FIS
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FisRegH2D {
    pub fis_type: u8,      // FIS_TYPE_REG_H2D
    pub pmport_c: u8,      // [7:4] PM Port, [7] C bit (1=command)
    pub command: u8,       // Command register
    pub featurel: u8,      // Feature register low
    pub lba0: u8,          // LBA 7:0
    pub lba1: u8,          // LBA 15:8
    pub lba2: u8,          // LBA 23:16
    pub device: u8,        // Device register
    pub lba3: u8,          // LBA 31:24
    pub lba4: u8,          // LBA 39:32
    pub lba5: u8,          // LBA 47:40
    pub featureh: u8,      // Feature register high
    pub countl: u8,        // Sector count low
    pub counth: u8,        // Sector count high
    pub icc: u8,           // Isochronous command
    pub control: u8,       // Control register
    pub rsv: [u8; 4],
}

/// Command header (one per slot in command list)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct CmdHeader {
    pub flags: u16,        // [4:0] CFL, [5] A, [6] W, [7] P, etc.
    pub prdtl: u16,        // PRDT length (entries)
    pub prdbc: u32,        // PRD byte count (result)
    pub ctba: u32,         // Command table base address low
    pub ctbau: u32,        // Command table base address high
    pub rsv: [u32; 4],
}

/// PRDT entry (Physical Region Descriptor Table)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PrdtEntry {
    pub dba: u32,          // Data base address low
    pub dbau: u32,         // Data base address high
    pub rsv: u32,
    pub dbc_i: u32,        // [21:0] byte count, [31] interrupt
}

/// Command table
#[repr(C)]
pub struct CmdTable {
    pub cfis: [u8; 64],    // Command FIS
    pub acmd: [u8; 16],    // ATAPI command
    pub rsv: [u8; 48],
    pub prdt: [PrdtEntry; 8], // PRDT entries (up to 8 for simplicity)
}

impl FisRegH2D {
    pub fn new_command(cmd: u8) -> Self {
        Self {
            fis_type: FIS_TYPE_REG_H2D,
            pmport_c: 0x80, // Command bit set
            command: cmd,
            featurel: 0,
            lba0: 0, lba1: 0, lba2: 0,
            device: 0x40, // LBA mode
            lba3: 0, lba4: 0, lba5: 0,
            featureh: 0,
            countl: 0, counth: 0,
            icc: 0, control: 0,
            rsv: [0; 4],
        }
    }

    pub fn set_lba(&mut self, lba: u64, count: u16) {
        self.lba0 = (lba & 0xFF) as u8;
        self.lba1 = ((lba >> 8) & 0xFF) as u8;
        self.lba2 = ((lba >> 16) & 0xFF) as u8;
        self.lba3 = ((lba >> 24) & 0xFF) as u8;
        self.lba4 = ((lba >> 32) & 0xFF) as u8;
        self.lba5 = ((lba >> 40) & 0xFF) as u8;
        self.countl = (count & 0xFF) as u8;
        self.counth = ((count >> 8) & 0xFF) as u8;
    }
}
