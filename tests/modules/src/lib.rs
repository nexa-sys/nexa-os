//! NexaOS Kernel Modules Test Suite
//!
//! 测试可加载内核模块：
//! - ext2/ext3/ext4: 文件系统驱动
//! - e1000: Intel 网卡驱动
//! - virtio_blk/virtio_net: VirtIO 驱动
//! - ahci/nvme: 存储控制器驱动

pub mod ext2;
pub mod ext3;
pub mod ext4;
pub mod e1000;
pub mod virtio_blk;
pub mod virtio_net;
pub mod ahci;
pub mod nvme;
