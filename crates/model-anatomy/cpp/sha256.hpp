// SHA-256 (FIPS 180-4), for the canonical space's content commitments.
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>

namespace ma {

class Sha256 {
public:
    Sha256();
    void update(const void* data, size_t len);
    void update(std::string_view text) { update(text.data(), text.size()); }
    std::array<uint8_t, 32> finish();
    std::string hex_digest();  // lowercase, as hashlib's hexdigest()

private:
    void block(const uint8_t* chunk);
    std::array<uint32_t, 8> state_;
    std::array<uint8_t, 64> buffer_{};
    size_t buffered_ = 0;
    uint64_t total_bytes_ = 0;
};

std::string sha256_hex(std::string_view data);

}  // namespace ma
