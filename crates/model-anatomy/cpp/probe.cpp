#include "probe.hpp"

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "json_writer.hpp"
#include "pyfmt.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <dxgi.h>
#include <pdh.h>
#include <pdhmsg.h>
#endif

namespace ma {

namespace {

struct Adapter {
    std::string name;
    uint32_t vendor_id = 0;
    uint32_t device_id = 0;
    int64_t dedicated_bytes = 0;
    bool software = false;
    // The adapter's locally unique id, which names it among Windows' GPU performance counters.
    uint32_t luid_high = 0;
    uint32_t luid_low = 0;
};

struct Probe {
    std::string backend = "absent";
    std::optional<Adapter> chosen;
    std::vector<Adapter> adapters;
    std::optional<int64_t> ram_total;
    std::optional<int64_t> ram_available;
    std::optional<int64_t> cpu_logical;
    std::optional<int64_t> cpu_physical;
    std::optional<int64_t> disk_free;
    // The chosen adapter's dedicated memory held now by every process on the machine, and what that leaves free.
    std::optional<int64_t> gpu_in_use;
    std::optional<int64_t> gpu_free;
    std::string platform;
    std::vector<std::string> gaps;
};

std::string hex4(uint32_t value) {
    static const char* const kHex = "0123456789abcdef";
    std::string out = "0x";
    bool started = false;
    for (int shift = 28; shift >= 0; shift -= 4) {
        const uint32_t digit = (value >> shift) & 15u;
        if (digit != 0 || started || shift < 16) {
            out += kHex[digit];
            started = true;
        }
    }
    return out;
}

// One hexadecimal device id, with or without "0x", ASCII whitespace around.
std::optional<uint32_t> parse_device_id(std::string_view text) {
    text = py_strip(text);
    if (text.size() > 2 && text[0] == '0' && (text[1] == 'x' || text[1] == 'X')) text.remove_prefix(2);
    if (text.empty() || text.size() > 8) return std::nullopt;
    uint32_t value = 0;
    for (const char c : text) {
        uint32_t digit = 0;
        if (c >= '0' && c <= '9') {
            digit = static_cast<uint32_t>(c - '0');
        } else if (c >= 'a' && c <= 'f') {
            digit = static_cast<uint32_t>(c - 'a' + 10);
        } else if (c >= 'A' && c <= 'F') {
            digit = static_cast<uint32_t>(c - 'A' + 10);
        } else {
            return std::nullopt;
        }
        value = value * 16 + digit;
    }
    return value;
}

#ifdef _WIN32

std::string utf8_of(const wchar_t* text, int length) {
    if (length == 0) return std::string();
    const int size = WideCharToMultiByte(CP_UTF8, 0, text, length, nullptr, 0, nullptr, nullptr);
    if (size <= 0) return std::string();
    std::string out(static_cast<size_t>(size), '\0');
    WideCharToMultiByte(CP_UTF8, 0, text, length, out.data(), size, nullptr, nullptr);
    return out;
}

std::optional<std::wstring> wide_of(const std::string& text) {
    if (text.empty()) return std::wstring();
    const int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()),
                                         nullptr, 0);
    if (size <= 0) return std::nullopt;
    std::wstring out(static_cast<size_t>(size), L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()), out.data(), size);
    return out;
}

std::string windows_error(DWORD code) { return "Windows error " + std::to_string(code); }

void read_adapters(Probe& probe) {
    IDXGIFactory1* factory = nullptr;
    const HRESULT made = CreateDXGIFactory1(__uuidof(IDXGIFactory1), reinterpret_cast<void**>(&factory));
    if (FAILED(made) || factory == nullptr) {
        probe.gaps.push_back("DXGI could not enumerate the display adapters (HRESULT " +
                             hex4(static_cast<uint32_t>(made)) + "); accelerator memory is UNMEASURED");
        return;
    }
    for (UINT index = 0;; ++index) {
        IDXGIAdapter1* adapter = nullptr;
        if (factory->EnumAdapters1(index, &adapter) == DXGI_ERROR_NOT_FOUND || adapter == nullptr) break;
        DXGI_ADAPTER_DESC1 desc{};
        if (SUCCEEDED(adapter->GetDesc1(&desc))) {
            Adapter item;
            int length = 0;
            while (length < 128 && desc.Description[length] != L'\0') ++length;
            item.name = utf8_of(desc.Description, length);
            item.vendor_id = desc.VendorId;
            item.device_id = desc.DeviceId;
            item.dedicated_bytes = static_cast<int64_t>(desc.DedicatedVideoMemory);
            item.software = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0;
            item.luid_high = static_cast<uint32_t>(desc.AdapterLuid.HighPart);
            item.luid_low = static_cast<uint32_t>(desc.AdapterLuid.LowPart);
            probe.adapters.push_back(item);
        }
        adapter->Release();
    }
    factory->Release();
}

// The chosen adapter's free memory: its dedicated memory less what every process holds on it now, read from the
// counter Task Manager reads (`\GPU Adapter Memory(luid_..._phys_0)\Dedicated Usage`, one sample, no device opened).
// Python asks torch (`mem_get_info`), which answers for the whole device too; DXGI's QueryVideoMemoryInfo is this
// process's budget, a different quantity, and is not read.
void read_adapter_use(Probe& probe) {
    if (!probe.chosen) return;
    wchar_t path[128];
    swprintf(path, 128, L"\\GPU Adapter Memory(luid_0x%08X_0x%08X_phys_0)\\Dedicated Usage", probe.chosen->luid_high,
             probe.chosen->luid_low);
    const auto unread = [&probe](const std::string& why) {
        probe.gaps.push_back("free accelerator memory is UNMEASURED: the GPU Adapter Memory counter for the chosen "
                             "adapter did not read (" + why + ")");
    };
    PDH_HQUERY query = nullptr;
    PDH_STATUS status = PdhOpenQueryW(nullptr, 0, &query);
    if (status != ERROR_SUCCESS) {
        unread("PDH status " + hex4(static_cast<uint32_t>(status)));
        return;
    }
    PDH_HCOUNTER counter = nullptr;
    PDH_FMT_COUNTERVALUE value{};
    status = PdhAddEnglishCounterW(query, path, 0, &counter);
    if (status == ERROR_SUCCESS) status = PdhCollectQueryData(query);
    if (status == ERROR_SUCCESS) status = PdhGetFormattedCounterValue(counter, PDH_FMT_LARGE, nullptr, &value);
    PdhCloseQuery(query);
    if (status != ERROR_SUCCESS) {
        unread("PDH status " + hex4(static_cast<uint32_t>(status)));
        return;
    }
    const int64_t in_use = value.largeValue;
    if (in_use < 0 || in_use > probe.chosen->dedicated_bytes) {
        unread("it said " + std::to_string(in_use) + " bytes in use of " +
               std::to_string(probe.chosen->dedicated_bytes));
        return;
    }
    probe.gpu_in_use = in_use;
    probe.gpu_free = probe.chosen->dedicated_bytes - in_use;
    probe.gaps.push_back(
        "free accelerator memory is the adapter's dedicated memory less the " + std::to_string(in_use) +
        " bytes every process held on it when read (the GPU Adapter Memory counter, one sample): it moves as programs "
        "allocate, and it is not a reservation");
}

void read_memory(Probe& probe) {
    MEMORYSTATUSEX status{};
    status.dwLength = sizeof(status);
    if (GlobalMemoryStatusEx(&status)) {
        probe.ram_total = static_cast<int64_t>(status.ullTotalPhys);
        probe.ram_available = static_cast<int64_t>(status.ullAvailPhys);
    } else {
        probe.gaps.push_back("system memory did not read (" + windows_error(GetLastError()) + ")");
    }
    const DWORD logical = GetActiveProcessorCount(ALL_PROCESSOR_GROUPS);
    if (logical != 0) probe.cpu_logical = static_cast<int64_t>(logical);
    DWORD length = 0;
    GetLogicalProcessorInformationEx(RelationProcessorCore, nullptr, &length);
    std::vector<unsigned char> buffer(length);
    if (length != 0 && GetLogicalProcessorInformationEx(
                           RelationProcessorCore,
                           reinterpret_cast<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX*>(buffer.data()), &length)) {
        int64_t cores = 0;
        for (DWORD at = 0; at < length;) {
            const auto* record = reinterpret_cast<const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX*>(buffer.data() + at);
            if (record->Size == 0) break;
            if (record->Relationship == RelationProcessorCore) ++cores;
            at += record->Size;
        }
        probe.cpu_physical = cores;
    }
    if (logical == 0 || !probe.cpu_physical) probe.gaps.emplace_back("the CPU count did not read");
}

void read_disk(Probe& probe, const std::string* root) {
    if (root == nullptr) {
        probe.gaps.emplace_back(
            "no home folder is named (USERPROFILE, else HOMEDRIVE and HOMEPATH), so free disk space is "
            "UNMEASURED");
        return;
    }
    const auto wide = wide_of(*root);
    ULARGE_INTEGER available{};
    ULARGE_INTEGER total{};
    ULARGE_INTEGER free{};
    if (wide && GetDiskFreeSpaceExW(wide->c_str(), &available, &total, &free)) {
        // CPython's `nt._getdiskusage` (shutil.disk_usage on Windows) passes on the third value, the volume's total
        // free bytes, not the bytes available to this caller.
        probe.disk_free = static_cast<int64_t>(free.QuadPart);
        return;
    }
    probe.gaps.push_back("free disk space at " + *root + " did not read (" + windows_error(GetLastError()) +
                         "); weight-download room is UNMEASURED");
}

void read_platform(Probe& probe) {
    using RtlGetVersionFn = LONG(WINAPI*)(OSVERSIONINFOW*);
    const HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    const FARPROC proc = ntdll == nullptr ? nullptr : GetProcAddress(ntdll, "RtlGetVersion");
    if (proc == nullptr) {
        probe.gaps.emplace_back("the platform string did not read");
        return;
    }
    const auto get_version = reinterpret_cast<RtlGetVersionFn>(reinterpret_cast<void*>(proc));
    OSVERSIONINFOW info{};
    info.dwOSVersionInfoSize = sizeof(info);
    if (get_version(&info) != 0) {
        probe.gaps.emplace_back("the platform string did not read");
        return;
    }
    probe.platform = "Windows " + std::to_string(info.dwMajorVersion) + "." + std::to_string(info.dwMinorVersion) +
                     "." + std::to_string(info.dwBuildNumber);
}

#endif

void choose(Probe& probe, const std::string* device_filter) {
    if (device_filter == nullptr) {
        probe.gaps.emplace_back(
            "VK_LOADER_DEVICE_ID_FILTER is unset, so no adapter is chosen: accelerator memory is UNMEASURED (the "
            "probe never falls back to the first adapter)");
        return;
    }
    const auto wanted = parse_device_id(*device_filter);
    if (!wanted) {
        probe.gaps.push_back("VK_LOADER_DEVICE_ID_FILTER is \"" + *device_filter +
                             "\", which is not one hexadecimal device id: accelerator memory is UNMEASURED");
        return;
    }
    std::vector<const Adapter*> matches;
    for (const auto& adapter : probe.adapters) {
        if (adapter.device_id == *wanted) matches.push_back(&adapter);
    }
    if (matches.empty()) {
        probe.gaps.push_back("no adapter DXGI enumerated has device id " + hex4(*wanted) +
                             " (VK_LOADER_DEVICE_ID_FILTER): accelerator memory is UNMEASURED");
        return;
    }
    if (matches.size() > 1) {
        probe.gaps.push_back(std::to_string(matches.size()) + " adapters have device id " + hex4(*wanted) +
                             ", so which one the filter means is not decided here: accelerator memory is UNMEASURED");
        return;
    }
    probe.chosen = *matches.front();
    probe.backend = "vulkan";
    probe.gaps.push_back(
        "the backend \"vulkan\" names the adapter VK_LOADER_DEVICE_ID_FILTER chooses (decided "
        "2026-10-07); DXGI enumerated it and no Vulkan instance or device was created, so Vulkan's own "
        "availability on it is not established here");
}

}  // namespace

namespace {

// The counter set, its package instance and counter, as Windows names them.
constexpr const char* kEnergyCounterSet = "Energy Meter";
constexpr const char* kEnergyInstance = "RAPL_Package0_PKG";
constexpr const char* kEnergyCounter = "Energy";

struct EnergySample {
    int64_t picowatt_hours = 0;
    int64_t filetime_100ns = 0;  // the sample's FILETIME (100 ns since 1601, UTC)
};

#ifdef _WIN32
// One raw sample. The counter is a cumulative count (PERF_COUNTER_LARGE_RAWCOUNT),
// so its raw value is the reading; the formatted value would be the same
// number through a double.
std::optional<EnergySample> read_energy(std::string& reason) {
    PDH_HQUERY query = nullptr;
    PDH_STATUS status = PdhOpenQueryW(nullptr, 0, &query);
    if (status != ERROR_SUCCESS) {
        reason = "PDH could not open a query (status " + hex4(static_cast<uint32_t>(status)) + ")";
        return std::nullopt;
    }
    PDH_HCOUNTER counter = nullptr;
    status = PdhAddEnglishCounterW(query, L"\\Energy Meter(RAPL_Package0_PKG)\\Energy", 0, &counter);
    if (status == static_cast<PDH_STATUS>(PDH_CSTATUS_NO_OBJECT)) {
        PdhCloseQuery(query);
        reason = std::string("this machine has no \"") + kEnergyCounterSet + "\" counter set";
        return std::nullopt;
    }
    if (status == static_cast<PDH_STATUS>(PDH_CSTATUS_NO_INSTANCE)) {
        PdhCloseQuery(query);
        reason = std::string("the \"") + kEnergyCounterSet + "\" counter set has no instance " + kEnergyInstance;
        return std::nullopt;
    }
    if (status == static_cast<PDH_STATUS>(PDH_CSTATUS_NO_COUNTER)) {
        PdhCloseQuery(query);
        reason = std::string("the \"") + kEnergyCounterSet + "\" counter set has no " + kEnergyCounter + " counter";
        return std::nullopt;
    }
    if (status == ERROR_SUCCESS) status = PdhCollectQueryData(query);
    PDH_RAW_COUNTER raw{};
    DWORD type = 0;
    if (status == ERROR_SUCCESS) status = PdhGetRawCounterValue(counter, &type, &raw);
    PdhCloseQuery(query);
    if (status != ERROR_SUCCESS) {
        reason = "the counter did not read (PDH status " + hex4(static_cast<uint32_t>(status)) + ")";
        return std::nullopt;
    }
    if (raw.CStatus != PDH_CSTATUS_VALID_DATA && raw.CStatus != PDH_CSTATUS_NEW_DATA) {
        reason = "the counter's sample is not valid (status " + hex4(static_cast<uint32_t>(raw.CStatus)) + ")";
        return std::nullopt;
    }
    if (raw.FirstValue < 0) {
        reason = "the counter read a negative cumulative energy (" + std::to_string(raw.FirstValue) + ")";
        return std::nullopt;
    }
    EnergySample sample;
    sample.picowatt_hours = raw.FirstValue;
    sample.filetime_100ns = (static_cast<int64_t>(raw.TimeStamp.dwHighDateTime) << 32) |
                            static_cast<int64_t>(raw.TimeStamp.dwLowDateTime);
    return sample;
}
#endif

}  // namespace

std::string cpu_package_energy_json() {
    std::string reason;
    std::optional<EnergySample> sample;
#ifdef _WIN32
    sample = read_energy(reason);
#else
    reason = "the CPU energy counter is read on Windows only";
#endif
    JsonWriter json;
    json.begin_object();
    json.key("energy");
    if (sample) {
        json.begin_object();
        json.key("picowatt_hours");
        json.integer(sample->picowatt_hours);
        json.key("counter_set");
        json.string(kEnergyCounterSet);
        json.key("instance");
        json.string(kEnergyInstance);
        json.key("counter");
        json.string(kEnergyCounter);
        json.key("filetime_100ns");
        json.integer(sample->filetime_100ns);
        json.end_object();
    } else {
        json.null();
    }
    json.key("reason");
    json.string(reason);
    json.end_object();
    return json.text();
}

std::string probe_json(const std::string* device_filter, const std::string* disk_root) {
    Probe probe;
#ifdef _WIN32
    read_adapters(probe);
    choose(probe, device_filter);
    read_adapter_use(probe);
    read_memory(probe);
    read_disk(probe, disk_root);
    read_platform(probe);
#else
    (void)disk_root;
    probe.gaps.emplace_back("the native probe reads Windows only; on this platform every machine fact is UNMEASURED");
    choose(probe, nullptr);
#endif
    probe.gaps.emplace_back(
        "the accelerator's architecture, compute units and bfloat16 support are UNMEASURED: reading them would "
        "open a device, which this probe never does");

    JsonWriter json;
    json.begin_object();
    json.key("probe");
    json.string("dxgi");
    json.key("backend");
    json.string(probe.backend);
    json.key("torch_version");
    json.string("");
    json.key("gpu");
    if (probe.chosen) {
        json.begin_object();
        json.key("name");
        json.string(probe.chosen->name);
        json.key("total_bytes");
        json.integer(probe.chosen->dedicated_bytes);
        json.key("free_bytes");
        json.optional_integer(probe.gpu_free);
        json.key("vendor_id");
        json.integer(probe.chosen->vendor_id);
        json.key("device_id");
        json.integer(probe.chosen->device_id);
        json.end_object();
    } else {
        json.null();
    }
    json.key("ram_total_bytes");
    json.optional_integer(probe.ram_total);
    json.key("ram_available_bytes");
    json.optional_integer(probe.ram_available);
    json.key("cpu_logical");
    json.optional_integer(probe.cpu_logical);
    json.key("cpu_physical");
    json.optional_integer(probe.cpu_physical);
    json.key("disk_free_bytes");
    json.optional_integer(probe.disk_free);
    json.key("disk_root");
    json.optional_string(disk_root == nullptr ? std::nullopt : std::optional<std::string>(*disk_root));
    json.key("bf16");
    json.null();
    json.key("platform");
    json.string(probe.platform);
    json.key("device_filter");
    json.optional_string(device_filter == nullptr ? std::nullopt : std::optional<std::string>(*device_filter));
    json.key("adapters");
    json.begin_array();
    for (const auto& adapter : probe.adapters) {
        json.begin_object();
        json.key("name");
        json.string(adapter.name);
        json.key("vendor_id");
        json.integer(adapter.vendor_id);
        json.key("device_id");
        json.integer(adapter.device_id);
        json.key("dedicated_bytes");
        json.integer(adapter.dedicated_bytes);
        json.key("software");
        json.boolean(adapter.software);
        json.end_object();
    }
    json.end_array();
    json.key("gaps");
    json.begin_array();
    for (const auto& gap : probe.gaps) json.string(gap);
    json.end_array();
    json.end_object();
    return json.text();
}

}  // namespace ma
