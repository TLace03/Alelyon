#include "constants.hpp"

#include <algorithm>

namespace ma {

const ModuleInfo& module_info(std::string_view id) {
    for (const auto& info : kModules) {
        if (info.id == id) return info;
    }
    return kModules.back();  // MODULES["other"]
}

const BitsPerWeight* nominal_bits(std::string_view type) {
    for (const auto& entry : kNominalBitsPerWeight) {
        if (entry.type == type) return &entry;
    }
    return nullptr;
}

int64_t stage_count() {
    int64_t highest = 0;
    for (const auto& info : kModules) highest = std::max(highest, info.stage);
    return highest + 1;
}

std::vector<std::string> module_ids() {
    std::vector<std::string> ids;
    for (const auto& info : kModules) ids.emplace_back(info.id);
    std::sort(ids.begin(), ids.end());
    return ids;
}

size_t family_index(std::string_view family) {
    for (size_t i = 0; i < kFamilies.size(); ++i) {
        if (kFamilies[i] == family) return i;
    }
    return kFamilies.size() - 1;
}

}  // namespace ma
