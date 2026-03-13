// Discord Voice Helper — lightweight C++ process using DPP
//
// Connects to a voice channel, records per-user PCM audio,
// and writes WAV files to an output directory.
//
// Communicates with the Node.js bridge via JSON lines on stdin/stdout.
//
// Usage:
//   voice-helper <bot-token> <guild-id> <channel-id> <output-dir>
//
// Protocol (stdout JSON lines):
//   {"type":"joined"}                                    — joined voice
//   {"type":"audio","user":"name","userId":"id","file":"path"} — audio saved
//   {"type":"error","message":"..."}                     — error
//   {"type":"left"}                                      — disconnected
//
// Protocol (stdin):
//   stop\n  — leave voice and exit

#include <dpp/dpp.h>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <map>
#include <mutex>
#include <string>
#include <thread>
#include <vector>
#include <filesystem>

namespace fs = std::filesystem;

// WAV header for 16-bit 48kHz stereo PCM
#pragma pack(push, 1)
struct WavHeader {
    char riff[4] = {'R','I','F','F'};
    uint32_t fileSize = 0;
    char wave[4] = {'W','A','V','E'};
    char fmt[4] = {'f','m','t',' '};
    uint32_t fmtSize = 16;
    uint16_t audioFormat = 1;
    uint16_t numChannels = 2;
    uint32_t sampleRate = 48000;
    uint32_t byteRate = 48000 * 2 * 2;
    uint16_t blockAlign = 4;
    uint16_t bitsPerSample = 16;
    char data[4] = {'d','a','t','a'};
    uint32_t dataSize = 0;
};
#pragma pack(pop)

static void send_json(const std::string& json) {
    std::cout << json << "\n" << std::flush;
}

static void send_event(const std::string& type) {
    send_json("{\"type\":\"" + type + "\"}");
}

static void send_error(const std::string& msg) {
    std::string escaped;
    for (char c : msg) {
        if (c == '"') escaped += "\\\"";
        else if (c == '\\') escaped += "\\\\";
        else if (c == '\n') escaped += "\\n";
        else escaped += c;
    }
    send_json("{\"type\":\"error\",\"message\":\"" + escaped + "\"}");
}

struct UserAudio {
    std::vector<uint8_t> samples;
    std::string username;
};

int main(int argc, char* argv[]) {
    if (argc < 5) {
        std::cerr << "Usage: voice-helper <bot-token> <guild-id> <channel-id> <output-dir>\n";
        return 1;
    }

    const std::string token = argv[1];
    const dpp::snowflake guild_id = std::stoull(argv[2]);
    const dpp::snowflake channel_id = std::stoull(argv[3]);
    const std::string output_dir = argv[4];

    fs::create_directories(output_dir);

    std::map<dpp::snowflake, UserAudio> user_audio;
    std::mutex audio_mutex;
    std::atomic<bool> running{true};

    dpp::cluster bot(token, dpp::i_guilds | dpp::i_guild_voice_states);

    // Forward DPP logs to stderr, but suppress noisy decrypt errors
    bot.on_log([](const dpp::log_t& event) {
        if (event.message.find("decrypt failed") != std::string::npos) return;
        std::cerr << "[dpp] " << event.message << "\n";
    });

    // on_guild_create fires when the guild enters cache (after on_ready)
    bot.on_guild_create([&](const dpp::guild_create_t& event) {
        if (event.created.id != guild_id) return;

        // Connect to voice via the shard's discord_client
        auto* shard = event.from();
        if (shard) {
            shard->connect_voice(guild_id, channel_id, false, false, true);
            // self_mute=false, self_deaf=false, enable_dave=true
        } else {
            send_error("No shard available");
            bot.shutdown();
        }
    });

    bot.on_voice_ready([&](const dpp::voice_ready_t& event) {
        // Send 0.5s of silence to prime Discord into sending us audio.
        auto* vc = event.voice_client;
        if (vc) {
            constexpr size_t silence_samples = 48000 / 2; // 0.5s at 48kHz
            constexpr size_t frame_bytes = silence_samples * 2 * 2; // stereo 16-bit
            std::vector<uint8_t> silence(frame_bytes, 0);
            vc->send_audio_raw(reinterpret_cast<uint16_t*>(silence.data()), silence.size());
            std::cerr << "[voice] Sent 0.5s silence to prime audio receive\n";
        }
        send_event("joined");
    });

    bot.on_voice_receive([&](const dpp::voice_receive_t& event) {
        if (!running.load()) return;
        // Skip bot's own audio (user_id 0 = unknown/self)
        if (event.user_id == 0 || event.user_id == bot.me.id) return;
        if (event.audio_data.empty()) return;

        // Filter out corrupt frames from DAVE decrypt failures.
        // Corrupt frames have most samples at extreme values (clipping).
        const auto* samples = reinterpret_cast<const int16_t*>(event.audio_data.data());
        size_t num_samples = event.audio_data.size() / 2;
        if (num_samples == 0) return;

        size_t extreme_count = 0;
        for (size_t i = 0; i < num_samples; i++) {
            int16_t s = samples[i];
            if (s > 30000 || s < -30000) extreme_count++;
        }

        // If >50% of samples are extreme, this frame is corrupt
        if (extreme_count * 2 > num_samples) return;

        std::lock_guard<std::mutex> lock(audio_mutex);
        auto& ua = user_audio[event.user_id];
        ua.samples.insert(ua.samples.end(), event.audio_data.begin(), event.audio_data.end());
    });

    // Start bot in background thread
    std::thread bot_thread([&]() {
        bot.start(dpp::st_wait);
    });

    // Read stdin for stop command
    std::string line;
    while (std::getline(std::cin, line)) {
        if (line == "stop") {
            running.store(false);

            // Resolve usernames and save audio
            {
                std::lock_guard<std::mutex> lock(audio_mutex);
                for (auto& [uid, ua] : user_audio) {
                    if (ua.samples.empty()) continue;

                    // Use user ID as filename if no username resolved
                    std::string name = std::to_string(uid);

                    // Try to get username from cache
                    dpp::user* u = dpp::find_user(uid);
                    if (u) {
                        name = u->global_name.empty() ? u->username : u->global_name;
                    }

                    // Sanitize filename
                    std::string safe_name;
                    for (char c : name) {
                        if (std::isalnum(c) || c == '-' || c == '_') safe_name += c;
                        else safe_name += '_';
                    }

                    std::string filepath = output_dir + "/" + safe_name + ".wav";
                    std::cerr << "[voice] Saving " << ua.samples.size() << " bytes for user " << name << "\n";

                    // Write WAV file
                    WavHeader header;
                    header.dataSize = static_cast<uint32_t>(ua.samples.size());
                    header.fileSize = 36 + header.dataSize;

                    std::ofstream out(filepath, std::ios::binary);
                    out.write(reinterpret_cast<const char*>(&header), sizeof(header));
                    out.write(reinterpret_cast<const char*>(ua.samples.data()), ua.samples.size());
                    out.close();

                    send_json("{\"type\":\"audio\",\"user\":\"" + name +
                              "\",\"userId\":\"" + std::to_string(uid) +
                              "\",\"file\":\"" + filepath + "\"}");
                }
            }

            // Disconnect — bot.shutdown() handles voice cleanup
            send_event("left");
            bot.shutdown();
            break;
        }
    }

    if (bot_thread.joinable()) {
        bot_thread.join();
    }

    return 0;
}
