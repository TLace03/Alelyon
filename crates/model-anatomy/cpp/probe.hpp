// The native machine probe (PR 3, deviation D10): what `workstation.read_workstation`
// reads through torch, psutil, shutil and platform, read here through Windows itself.
//
// It never opens a device. The accelerator is found by DXGI enumeration only
// (CreateDXGIFactory1, EnumAdapters1, GetDesc1) and chosen by the device id in
// VK_LOADER_DEVICE_ID_FILTER, the variable that confines this machine's Vulkan
// work to the RX; unset, unparsable or matching no adapter (or two) leaves
// accelerator memory UNMEASURED with a gap, never the first adapter.
#pragma once

#include <string>

namespace ma {

// The reading as JSON. `device_filter` and `disk_root` null are absent.
std::string probe_json(const std::string* device_filter, const std::string* disk_root);

// The CPU package's cumulative energy (PR 5): one raw sample of Windows' counter
// `\Energy Meter(RAPL_Package0_PKG)\Energy`, in picowatt-hours, with the
// sample's own timestamp. No device is opened. Absent counter set or instance:
// `energy` is null and `reason` names why.
std::string cpu_package_energy_json();

}  // namespace ma
