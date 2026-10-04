// DirectX 11 frame adapter. DllMain performs no hook installation or unloading.
#include <windows.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <atomic>
#include <filesystem>
#include <fstream>
#include <vector>
#include <mutex>
#include <string>
using Microsoft::WRL::ComPtr;
namespace fs = std::filesystem;
using PresentFn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
using Present1Fn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain1*, UINT, UINT, const DXGI_PRESENT_PARAMETERS*);
static PresentFn original_present = nullptr;
static Present1Fn original_present1 = nullptr;
static void* target_present = nullptr;
static void* target_present1 = nullptr;
static std::atomic_bool started{false}, requested{false};
static std::atomic_bool render_paused{false};
static std::atomic<DWORD> pause_owner{0};
static std::atomic_uint pause_threads{0};
static std::mutex capture_lock;
static fs::path session;

// Publishes a complete status file; readers never see a partially written frame.
static void publish(const fs::path& path, const std::string& value) {
    auto temporary = path; temporary += L".tmp";
    { std::ofstream out(temporary, std::ios::binary); out << value; }
    MoveFileExW(temporary.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH);
}

// Copies the swap chain back buffer without changing any game pipeline state.
static void capture(IDXGISwapChain* chain) {
    ComPtr<ID3D11Device> device;
    if (FAILED(chain->GetDevice(IID_PPV_ARGS(&device)))) throw std::runtime_error("Swap chain is not DirectX 11");
    ComPtr<ID3D11DeviceContext> context; device->GetImmediateContext(&context);
    ComPtr<ID3D11Texture2D> source;
    if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&source)))) throw std::runtime_error("GetBuffer failed");
    D3D11_TEXTURE2D_DESC desc{}; source->GetDesc(&desc);
    if (!desc.Width || !desc.Height || desc.Width > 16384 || desc.Height > 16384) throw std::runtime_error("Invalid frame dimensions");
    const bool bgra = desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM || desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB || desc.Format == DXGI_FORMAT_B8G8R8X8_UNORM;
    const bool rgba = desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM || desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
    const bool hdr10 = desc.Format == DXGI_FORMAT_R10G10B10A2_UNORM;
    if (!bgra && !rgba && !hdr10) throw std::runtime_error("Unsupported frame format: disable HDR / use SDR");
    if (desc.SampleDesc.Count > 1) {
        auto resolved_desc = desc; resolved_desc.SampleDesc = {1, 0}; resolved_desc.BindFlags = 0; resolved_desc.MiscFlags = 0;
        ComPtr<ID3D11Texture2D> resolved;
        if (FAILED(device->CreateTexture2D(&resolved_desc, nullptr, &resolved))) throw std::runtime_error("MSAA resolve allocation failed");
        context->ResolveSubresource(resolved.Get(), 0, source.Get(), 0, desc.Format);
        source = resolved; desc = resolved_desc;
    }
    auto staging_desc = desc; staging_desc.Usage = D3D11_USAGE_STAGING; staging_desc.BindFlags = 0;
    staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ; staging_desc.MiscFlags = 0;
    ComPtr<ID3D11Texture2D> staging;
    if (FAILED(device->CreateTexture2D(&staging_desc, nullptr, &staging))) throw std::runtime_error("Staging allocation failed");
    context->CopyResource(staging.Get(), source.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) throw std::runtime_error("Frame readback failed");
    // Always unmap, including allocation or file errors.
    struct Unmapper { ID3D11DeviceContext* c; ID3D11Texture2D* t; ~Unmapper(){ c->Unmap(t, 0); } } unmapper{context.Get(), staging.Get()};
    std::vector<unsigned char> pixels(static_cast<size_t>(desc.Width) * desc.Height * 4);
    for (UINT y = 0; y < desc.Height; ++y) {
        const auto* row = static_cast<const unsigned char*>(mapped.pData) + static_cast<size_t>(y) * mapped.RowPitch;
        auto* destination = pixels.data() + static_cast<size_t>(y) * desc.Width * 4;
        for (UINT x = 0; x < desc.Width; ++x) {
            if (hdr10) {
                UINT p; memcpy(&p, row + x * 4, 4);
                destination[x*4] = static_cast<unsigned char>(((p >> 20) & 1023) * 255 / 1023);
                destination[x*4+1] = static_cast<unsigned char>(((p >> 10) & 1023) * 255 / 1023);
                destination[x*4+2] = static_cast<unsigned char>((p & 1023) * 255 / 1023);
            } else {
                destination[x*4] = row[x*4 + (rgba ? 2 : 0)];
                destination[x*4+1] = row[x*4+1];
                destination[x*4+2] = row[x*4 + (rgba ? 0 : 2)];
            }
            destination[x*4+3] = 255;
        }
    }
    BITMAPFILEHEADER file{}; file.bfType = 0x4d42; file.bfOffBits = sizeof(file) + sizeof(BITMAPINFOHEADER);
    file.bfSize = file.bfOffBits + static_cast<DWORD>(pixels.size());
    BITMAPINFOHEADER info{}; info.biSize = sizeof(info); info.biWidth = static_cast<LONG>(desc.Width);
    info.biHeight = -static_cast<LONG>(desc.Height); info.biPlanes = 1; info.biBitCount = 32; info.biCompression = BI_RGB;
    auto temporary = session / L"frame.tmp";
    { std::ofstream out(temporary, std::ios::binary); out.write(reinterpret_cast<const char*>(&file), sizeof(file));
      out.write(reinterpret_cast<const char*>(&info), sizeof(info)); out.write(reinterpret_cast<const char*>(pixels.data()), pixels.size());
      if (!out) throw std::runtime_error("Could not save frame"); }
    if (!MoveFileExW(temporary.c_str(), (session / L"frame.bmp").c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) throw std::runtime_error("Could not publish frame");
    publish(session / L"frame.ready", "ok");
}

// Takes one requested frame, while preserving the original Present return value.
static void on_present(IDXGISwapChain* chain, UINT flags) noexcept {
    if (flags & DXGI_PRESENT_TEST || !requested.load()) return;
    std::unique_lock lock(capture_lock, std::try_to_lock);
    if (!lock || !requested.exchange(false)) return;
    try { capture(chain); } catch (const std::exception& error) { publish(session / L"frame.ready", std::string("error: ") + error.what()); }
    catch (...) { publish(session / L"frame.ready", "error: unexpected capture failure"); }
}
// Stops the render/game loop at Present while servicing the thread's Windows message queue.
static void pause_point(UINT flags) noexcept {
    static thread_local bool pumping=false;
    if(pumping || flags & DXGI_PRESENT_TEST || !render_paused.load())return;
    pumping=true;
    if(pause_threads.fetch_add(1)==0)publish(session/L"render-paused.status",std::to_string(pause_owner.load()));
    while(render_paused.load()) {
        MSG message{};
        for(int i=0;i<64&&PeekMessageW(&message,nullptr,0,0,PM_REMOVE);++i) {
            if(message.message==WM_QUIT){render_paused.store(false);PostQuitMessage(static_cast<int>(message.wParam));break;}
            TranslateMessage(&message);DispatchMessageW(&message);
        }
        MsgWaitForMultipleObjectsEx(0,nullptr,10,QS_ALLINPUT,MWMO_INPUTAVAILABLE);
    }
    if(pause_threads.fetch_sub(1)==1){std::error_code error;fs::remove(session/L"render-paused.status",error);}
    pumping=false;
}
static HRESULT STDMETHODCALLTYPE hooked_present(IDXGISwapChain* chain, UINT interval, UINT flags) {
    on_present(chain, flags);auto result=original_present(chain, interval, flags);pause_point(flags);return result;
}
static HRESULT STDMETHODCALLTYPE hooked_present1(IDXGISwapChain1* chain, UINT interval, UINT flags, const DXGI_PRESENT_PARAMETERS* parameters) {
    on_present(chain, flags);auto result=original_present1(chain, interval, flags, parameters);pause_point(flags);return result;
}

// Finds system DXGI entry points using an isolated hidden D3D11 device.
static void initialize() {
    auto hwnd = CreateWindowExW(0, L"STATIC", L"Translit probe", WS_POPUP, 0, 0, 2, 2, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    if (!hwnd) throw std::runtime_error("Probe window creation failed");
    struct WindowGuard { HWND h; ~WindowGuard(){DestroyWindow(h);} } guard{hwnd};
    DXGI_SWAP_CHAIN_DESC desc{}; desc.BufferCount = 1; desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.BufferDesc.Width = 2; desc.BufferDesc.Height = 2; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.OutputWindow = hwnd; desc.SampleDesc.Count = 1; desc.Windowed = TRUE; desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    ComPtr<IDXGISwapChain> chain; ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    auto hr = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr, 0, D3D11_SDK_VERSION, &desc, &chain, &device, nullptr, &context);
    if (FAILED(hr)) throw std::runtime_error("D3D11 probe creation failed");
    target_present = (*reinterpret_cast<void***>(chain.Get()))[8];
    ComPtr<IDXGISwapChain1> chain1;
    if (SUCCEEDED(chain.As(&chain1))) target_present1 = (*reinterpret_cast<void***>(chain1.Get()))[22];
    if (MH_Initialize() != MH_OK) throw std::runtime_error("MinHook initialization failed");
    if (MH_CreateHook(target_present, reinterpret_cast<void*>(&hooked_present), reinterpret_cast<void**>(&original_present)) != MH_OK) throw std::runtime_error("Present hook creation failed");
    if (target_present1 && target_present1 != target_present && MH_CreateHook(target_present1, reinterpret_cast<void*>(&hooked_present1), reinterpret_cast<void**>(&original_present1)) != MH_OK) target_present1 = nullptr;
    if (MH_EnableHook(target_present) != MH_OK) throw std::runtime_error("Present hook activation failed");
    if (target_present1) MH_EnableHook(target_present1);
}

// Handles control requests off the render thread. DLL remains loaded until game exit.
static DWORD WINAPI worker(void*) noexcept {
    wchar_t temporary[MAX_PATH]{}; GetTempPathW(MAX_PATH, temporary);
    session = fs::path(temporary) / (L"translit-native-v2-" + std::to_wstring(GetCurrentProcessId()));
    try {
        fs::create_directories(session); initialize(); publish(session / L"hook.status", "hooked");
        HANDLE pause_guard=nullptr;ULONGLONG pause_deadline=0;
        for (;;) {
            std::error_code error;
            if(fs::exists(session/L"render-pause.request",error)&&!render_paused.load()) {
                DWORD owner=0;{std::ifstream file(session/L"render-pause.request");file>>owner;}
                if(owner){if(pause_guard)CloseHandle(pause_guard);pause_guard=OpenProcess(SYNCHRONIZE,FALSE,owner);
                    if(pause_guard){pause_owner.store(owner);pause_deadline=GetTickCount64()+15*60*1000;render_paused.store(true);}}
            }
            if(fs::remove(session/L"render-resume.request",error)||
                (pause_guard&&(WaitForSingleObject(pause_guard,0)!=WAIT_TIMEOUT||GetTickCount64()>pause_deadline))) {
                render_paused.store(false);fs::remove(session/L"render-pause.request",error);
                if(pause_guard){CloseHandle(pause_guard);pause_guard=nullptr;}
            }
            if (fs::remove(session / L"capture.request", error)) requested.store(true);
            if (fs::remove(session / L"disable.request", error)) {
                render_paused.store(false);fs::remove(session/L"render-pause.request",error);
                MH_DisableHook(target_present); if (target_present1) MH_DisableHook(target_present1);
                requested.store(false); publish(session / L"hook.status", "disabled");
            }
            if (fs::remove(session / L"enable.request", error)) {
                MH_EnableHook(target_present); if (target_present1) MH_EnableHook(target_present1);
                publish(session / L"hook.status", "hooked");
            }
            Sleep(25);
        }
    } catch (const std::exception& error) { publish(session / L"hook.status", std::string("error: ") + error.what()); }
    catch (...) { publish(session / L"hook.status", "error: initialization failure"); }
    return 0;
}
extern "C" __declspec(dllexport) DWORD WINAPI TranslitStart(void*) {
    if (started.exchange(true)) return 1;
    auto thread = CreateThread(nullptr, 0, worker, nullptr, 0, nullptr);
    if (!thread) { started.store(false); return 0; }
    CloseHandle(thread); return 1;
}
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) DisableThreadLibraryCalls(instance);
    return TRUE;
}
