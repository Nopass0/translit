#include <windows.h>
#include <tlhelp32.h>
#include <wincred.h>
#include <winrt/base.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Globalization.h>
#include <winrt/Windows.Graphics.Imaging.h>
#include <winrt/Windows.Media.Ocr.h>
#include <winrt/Windows.Storage.h>
#include <winrt/Windows.Storage.Streams.h>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>
#include <algorithm>
#include <memory>
namespace fs = std::filesystem;
struct Handle {
    HANDLE value = nullptr;
    explicit Handle(HANDLE h = nullptr):value(h){}
    ~Handle(){ if(value && value != INVALID_HANDLE_VALUE) CloseHandle(value); }
    Handle(const Handle&) = delete; Handle& operator=(const Handle&) = delete;
};
static void require(bool condition, const char* message) { if(!condition) throw std::runtime_error(std::string(message) + " (Win32 " + std::to_string(GetLastError()) + ")"); }
static std::string utf8(const std::wstring& value) { return winrt::to_string(value); }
static std::string quote(const std::string& text) {
    std::string result = "\"";
    for (unsigned char c : text) {
        if(c == '"' || c == '\\') { result += '\\'; result += c; }
        else if(c == '\n') result += "\\n";
        else if(c == '\r') result += "\\r";
        else if(c == '\t') result += "\\t";
        else if(c < 32) { char buffer[7]; sprintf_s(buffer, "\\u%04x", c); result += buffer; }
        else result += c;
    }
    return result + '"';
}
static fs::path session(DWORD pid) {
    wchar_t temporary[MAX_PATH]{}; GetTempPathW(MAX_PATH, temporary);
    auto path = fs::path(temporary) / (L"translit-native-v2-" + std::to_wstring(pid)); fs::create_directories(path); return path;
}
static void write(const fs::path& path, const std::string& text) { std::ofstream out(path, std::ios::binary); out << text; require(static_cast<bool>(out), "File write failed"); }
static std::string read(const fs::path& path) { std::ifstream in(path, std::ios::binary); return {std::istreambuf_iterator<char>(in), {}}; }
static std::wstring event_name(DWORD pid, DWORD parent, const wchar_t* suffix) { return L"Local\\Translit." + std::to_wstring(pid) + L"." + std::to_wstring(parent) + L"." + suffix; }

// Enumerates visible top-level game windows, without opening game memory.
static BOOL CALLBACK enum_windows(HWND hwnd, LPARAM param) {
    if (!IsWindowVisible(hwnd) || !GetWindowTextLengthW(hwnd) || GetWindow(hwnd, GW_OWNER)) return TRUE;
    DWORD pid = 0; GetWindowThreadProcessId(hwnd, &pid);
    Handle process(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid)); if (!process.value) return TRUE;
    wchar_t path[32768]; DWORD size = 32768; if (!QueryFullProcessImageNameW(process.value, 0, path, &size)) return TRUE;
    auto filename = fs::path(path).filename().wstring();
    for(auto excluded:{L"translit.exe",L"explorer.exe",L"ApplicationFrameHost.exe",L"SystemSettings.exe",L"TabTip.exe",L"TextInputHost.exe",L"NVIDIA Overlay.exe"}) if(!_wcsicmp(filename.c_str(),excluded))return TRUE;
    RECT rect{}; GetClientRect(hwnd,&rect); POINT origin{0,0};ClientToScreen(hwnd,&origin);
    if(rect.right<320||rect.bottom<200)return TRUE;
    bool dx11=false;Handle snapshot(CreateToolhelp32Snapshot(TH32CS_SNAPMODULE|TH32CS_SNAPMODULE32,pid));
    MODULEENTRY32W module{};module.dwSize=sizeof(module);
    if(snapshot.value!=INVALID_HANDLE_VALUE&&Module32FirstW(snapshot.value,&module))do{if(!_wcsicmp(module.szModule,L"d3d11.dll"))dx11=true;}while(Module32NextW(snapshot.value,&module));
    USHORT machine=0,native=0;bool x64=IsWow64Process2(process.value,&machine,&native)&&machine==IMAGE_FILE_MACHINE_UNKNOWN&&native==IMAGE_FILE_MACHINE_AMD64;
    wchar_t title[1024]{}; GetWindowTextW(hwnd, title, 1024);
    auto& rows = *reinterpret_cast<std::vector<std::string>*>(param);
    rows.push_back("{\"pid\":" + std::to_string(pid) + ",\"hwnd\":" + std::to_string(reinterpret_cast<uintptr_t>(hwnd)) + ",\"name\":" + quote(utf8(filename)) + ",\"title\":" + quote(utf8(title)) + ",\"path\":" + quote(utf8(path)) + ",\"api\":"+quote(dx11?"dx11":"screen")+",\"x64\":"+(x64?"true":"false")+",\"x\":"+std::to_string(origin.x)+",\"y\":"+std::to_string(origin.y)+",\"width\":"+std::to_string(rect.right)+",\"height\":"+std::to_string(rect.bottom)+"}");
    return TRUE;
}
static void list() {
    std::vector<std::string> rows; EnumWindows(enum_windows, reinterpret_cast<LPARAM>(&rows));
    std::cout << '['; for(size_t i=0; i<rows.size(); ++i) { if(i) std::cout << ','; std::cout << rows[i]; } std::cout << ']';
}
// Refreshes just the attached window instead of scanning every process on each hotkey.
static void selected_window(HWND hwnd) {
    require(IsWindow(hwnd)&&!IsIconic(hwnd), "Game window is closed or minimized");
    std::vector<std::string> rows;
    enum_windows(hwnd,reinterpret_cast<LPARAM>(&rows));
    require(rows.size()==1,"Game window is unavailable");
    std::cout<<rows.front();
}

// Resolves module base in the selected process; does not assume equal ASLR bases.
static uintptr_t module_base(DWORD pid, const std::wstring& filename) {
    for(int retry=0; retry<20; ++retry) {
        Handle snapshot(CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid));
        if(snapshot.value != INVALID_HANDLE_VALUE) {
            MODULEENTRY32W entry{}; entry.dwSize = sizeof(entry);
            if(Module32FirstW(snapshot.value, &entry)) do {
                if(!_wcsicmp(entry.szModule, filename.c_str())) return reinterpret_cast<uintptr_t>(entry.modBaseAddr);
            } while(Module32NextW(snapshot.value, &entry));
        }
        Sleep(50);
    }
    return 0;
}
static void remote_call(HANDLE process, uintptr_t address, void* parameter) {
    Handle thread(CreateRemoteThread(process, nullptr, 0, reinterpret_cast<LPTHREAD_START_ROUTINE>(address), parameter, 0, nullptr));
    require(thread.value != nullptr, "CreateRemoteThread failed");
    require(WaitForSingleObject(thread.value, 10000) == WAIT_OBJECT_0, "Remote initialization timed out");
    DWORD exit = 0; require(GetExitCodeThread(thread.value, &exit) && exit != 0, "Remote initialization failed");
}

// Loads only our local DLL into the explicitly selected x64 game, then starts it.
static void inject(DWORD pid, const fs::path& dll) {
    Handle process(OpenProcess(PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ, FALSE, pid));
    require(process.value != nullptr, "OpenProcess failed");
    USHORT machine=0, native=0; require(IsWow64Process2(process.value, &machine, &native) && machine == IMAGE_FILE_MACHINE_UNKNOWN && native == IMAGE_FILE_MACHINE_AMD64, "Only native x64 games are supported");
    auto full = fs::absolute(dll).wstring(); require(fs::exists(full), "Hook DLL missing");
    auto directory = session(pid); auto filename = fs::path(full).filename().wstring();
    auto base = module_base(pid, filename);
    if(!base) {
        auto loader = GetProcAddress(GetModuleHandleW(L"kernel32.dll"), "LoadLibraryW"); require(loader != nullptr, "LoadLibrary resolution failed");
        HMODULE owner = nullptr;
        require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, reinterpret_cast<LPCWSTR>(loader), &owner), "Loader owner resolution failed");
        wchar_t owner_path[MAX_PATH]{}; GetModuleFileNameW(owner, owner_path, MAX_PATH);
        auto remote_owner = module_base(pid, fs::path(owner_path).filename().wstring()); require(remote_owner != 0, "Remote loader module missing");
        auto address = remote_owner + (reinterpret_cast<uintptr_t>(loader) - reinterpret_cast<uintptr_t>(owner));
        auto bytes = (full.size()+1)*sizeof(wchar_t);
        auto memory = VirtualAllocEx(process.value, nullptr, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE); require(memory != nullptr, "Remote path allocation failed");
        SIZE_T written=0;
        // A timeout retains the allocation: the remote loader might still be reading it.
        require(WriteProcessMemory(process.value, memory, full.c_str(), bytes, &written) && written == bytes, "Remote path write failed");
        remote_call(process.value, address, memory);
        VirtualFreeEx(process.value, memory, 0, MEM_RELEASE);
        base = module_base(pid, filename); require(base != 0, "Injected module not found");
    }
    auto local = LoadLibraryExW(full.c_str(), nullptr, DONT_RESOLVE_DLL_REFERENCES); require(local != nullptr, "Local DLL inspection failed");
    auto start = GetProcAddress(local, "TranslitStart");
    auto offset = reinterpret_cast<uintptr_t>(start) - reinterpret_cast<uintptr_t>(local); FreeLibrary(local);
    require(start != nullptr, "TranslitStart export missing");
    std::error_code error; fs::remove(directory / L"hook.status", error);
    remote_call(process.value, base + offset, nullptr);
    write(directory / L"enable.request", "1");
    for(int i=0; i<200; ++i) {
        auto status = read(directory / L"hook.status");
        if(status == "hooked") { std::cout << "ok"; return; }
        if(status.starts_with("error:")) throw std::runtime_error(status);
        Sleep(25);
    }
    throw std::runtime_error("Hook initialization timed out");
}

// A separate process owns suspension and automatically resumes on parent exit.
static void pause_watch(DWORD pid, DWORD parent, bool render=false) {
    Handle game(OpenProcess(PROCESS_SUSPEND_RESUME | SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid));
    Handle application(OpenProcess(SYNCHRONIZE, FALSE, parent));
    require(game.value && application.value, "Watchdog process access failed");
    Handle resume(CreateEventW(nullptr, TRUE, FALSE, event_name(pid,parent,L"resume").c_str()));
    require(resume.value != nullptr, "Watchdog event creation failed");
    if(render) {
        auto directory=session(pid);std::error_code error;
        fs::remove(directory/L"render-paused.status",error);fs::remove(directory/L"render-resume.request",error);
        struct RenderGuard{fs::path directory;~RenderGuard(){try{std::error_code error;fs::remove(directory/L"render-pause.request",error);write(directory/L"render-resume.request","1");}catch(...){}}} guard{directory};
        auto lease=std::to_string(GetCurrentProcessId());write(directory/L"render-pause.request",lease);
        bool ready=false;
        for(int i=0;i<300;++i){if(read(directory/L"render-paused.status")==lease){ready=true;break;}if(WaitForSingleObject(application.value,0)!=WAIT_TIMEOUT)break;Sleep(10);}
        require(ready,"Render pause did not reach Present");
        write(directory/L"paused.status",lease);
        HANDLE objects[]={resume.value,application.value,game.value};
        WaitForMultipleObjects(3,objects,FALSE,15*60*1000);
        fs::remove(directory/L"paused.status",error);return;
    }
    auto ntdll = GetModuleHandleW(L"ntdll.dll");
    using NtProcess = LONG(NTAPI*)(HANDLE);
    auto suspend = reinterpret_cast<NtProcess>(GetProcAddress(ntdll, "NtSuspendProcess"));
    auto restart = reinterpret_cast<NtProcess>(GetProcAddress(ntdll, "NtResumeProcess"));
    require(suspend && restart, "Process pause API unavailable");
    require(suspend(game.value) >= 0, "Could not pause game");
    struct ResumeGuard { NtProcess function; HANDLE process; ~ResumeGuard(){function(process);} } guard{restart,game.value};
    write(session(pid)/L"paused.status", std::to_string(GetCurrentProcessId()));
    HANDLE objects[] = {resume.value, application.value, game.value};
    WaitForMultipleObjects(3, objects, FALSE, 15*60*1000); // bounded pause lease, even if UI hangs
    std::error_code error; fs::remove(session(pid)/L"paused.status", error);
}
static void resume_game(DWORD pid, DWORD parent) {
    Handle event(OpenEventW(EVENT_MODIFY_STATE, FALSE, event_name(pid,parent,L"resume").c_str()));
    if(event.value) require(SetEvent(event.value), "Resume event failed");
}

// Captures visible client pixels for renderers without a compatible DLL adapter.
static void capture_window(HWND hwnd,const fs::path& destination) {
    require(IsWindow(hwnd)&&!IsIconic(hwnd),"Game window must be visible");
    RECT rect{};require(GetClientRect(hwnd,&rect),"Client rectangle unavailable");
    POINT origin{0,0};ClientToScreen(hwnd,&origin);
    require(rect.right>0&&rect.bottom>0,"Empty game window");
    HDC desktop=GetDC(nullptr),memory=CreateCompatibleDC(desktop);
    BITMAPINFO info{};info.bmiHeader.biSize=sizeof(BITMAPINFOHEADER);info.bmiHeader.biWidth=rect.right;info.bmiHeader.biHeight=-rect.bottom;info.bmiHeader.biPlanes=1;info.bmiHeader.biBitCount=32;
    void* pixels=nullptr;auto bitmap=CreateDIBSection(desktop,&info,DIB_RGB_COLORS,&pixels,nullptr,0);
    auto previous=SelectObject(memory,bitmap);
    bool ok=BitBlt(memory,0,0,rect.right,rect.bottom,desktop,origin.x,origin.y,SRCCOPY|CAPTUREBLT)!=0;
    if(ok){BITMAPFILEHEADER file{};file.bfType=0x4d42;file.bfOffBits=sizeof(file)+sizeof(BITMAPINFOHEADER);file.bfSize=file.bfOffBits+rect.right*rect.bottom*4;
        std::ofstream out(destination,std::ios::binary);out.write(reinterpret_cast<const char*>(&file),sizeof(file));out.write(reinterpret_cast<const char*>(&info.bmiHeader),sizeof(info.bmiHeader));out.write(static_cast<const char*>(pixels),static_cast<size_t>(rect.right)*rect.bottom*4);ok=static_cast<bool>(out);}
    SelectObject(memory,previous);DeleteObject(bitmap);DeleteDC(memory);ReleaseDC(nullptr,desktop);
    require(ok,"Visible-window capture failed");
}
// Credentials stay in Windows Credential Manager, not in vocabulary/settings JSON.
static void credential(const std::wstring& mode){
    wchar_t target[]=L"Translit.ContextTranslator";
    if(mode==L"key-set"){
        std::string key;std::getline(std::cin,key);require(!key.empty()&&key.size()<CRED_MAX_CREDENTIAL_BLOB_SIZE,"Invalid key length");
        CREDENTIALW value{};value.Type=CRED_TYPE_GENERIC;value.TargetName=target;value.CredentialBlobSize=static_cast<DWORD>(key.size());value.CredentialBlob=reinterpret_cast<BYTE*>(key.data());value.Persist=CRED_PERSIST_LOCAL_MACHINE;
        require(CredWriteW(&value,0),"Could not store API key");SecureZeroMemory(key.data(),key.size());
    }else if(mode==L"key-clear"){CredDeleteW(target,CRED_TYPE_GENERIC,0);}
    else{PCREDENTIALW value=nullptr;if(CredReadW(target,CRED_TYPE_GENERIC,0,&value)){if(mode==L"key-get")std::cout.write(reinterpret_cast<const char*>(value->CredentialBlob),value->CredentialBlobSize);else std::cout<<"saved";CredFree(value);}else if(mode!=L"key-get")std::cout<<"empty";}
}

// Runs Windows OCR locally and returns original-frame word coordinates as JSON.
static void ocr(const fs::path& image) {
    using namespace winrt::Windows;
    winrt::init_apartment(winrt::apartment_type::multi_threaded);
    auto language = Globalization::Language(L"en-US");
    auto engine = Media::Ocr::OcrEngine::TryCreateFromLanguage(language);
    if(!engine) throw std::runtime_error("English OCR language is not installed. Add English language and OCR in Windows Settings.");
    auto file = Storage::StorageFile::GetFileFromPathAsync(fs::absolute(image).wstring()).get();
    auto stream = file.OpenAsync(Storage::FileAccessMode::Read).get();
    auto decoder = Graphics::Imaging::BitmapDecoder::CreateAsync(stream).get();
    auto width = decoder.PixelWidth(), height = decoder.PixelHeight();
    auto maximum = Media::Ocr::OcrEngine::MaxImageDimension();
    auto scale = std::min(1.0, static_cast<double>(maximum) / std::max(width,height));
    Graphics::Imaging::BitmapTransform transform;
    transform.ScaledWidth(static_cast<uint32_t>(width * scale)); transform.ScaledHeight(static_cast<uint32_t>(height * scale));
    auto bitmap = decoder.GetSoftwareBitmapAsync(Graphics::Imaging::BitmapPixelFormat::Bgra8, Graphics::Imaging::BitmapAlphaMode::Ignore, transform, Graphics::Imaging::ExifOrientationMode::IgnoreExifOrientation, Graphics::Imaging::ColorManagementMode::DoNotColorManage).get();
    auto result = engine.RecognizeAsync(bitmap).get();
    std::cout << "{\"width\":" << width << ",\"height\":" << height << ",\"text\":" << quote(winrt::to_string(result.Text())) << ",\"words\":[";
    bool first = true; int line_id = 0;
    for(auto line : result.Lines()) {
        for(auto word : line.Words()) {
            auto box = word.BoundingRect(); if(!first) std::cout << ','; first=false;
            std::cout << "{\"text\":" << quote(winrt::to_string(word.Text())) << ",\"line\":" << line_id << ",\"x\":" << box.X/scale << ",\"y\":" << box.Y/scale << ",\"width\":" << box.Width/scale << ",\"height\":" << box.Height/scale << '}';
        }
        ++line_id;
    }
    std::cout << "]}";
}

int wmain(int argc, wchar_t** argv) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    try {
        if(argc<2) throw std::runtime_error("Expected list, inject, ocr, pause, resume, detach");
        std::wstring command = argv[1];
        if(command == L"list") list();
        else if(command==L"window"&&argc==3) selected_window(reinterpret_cast<HWND>(std::stoull(argv[2])));
        else if(command==L"screen"&&argc==4)capture_window(reinterpret_cast<HWND>(std::stoull(argv[2])),argv[3]);
        else if(command.starts_with(L"key-"))credential(command);
        else if(command == L"ocr" && argc==3) ocr(argv[2]);
        else if(command == L"inject" && argc==4) inject(std::stoul(argv[2]), argv[3]);
        else if(command == L"pause" && argc==4) pause_watch(std::stoul(argv[2]), std::stoul(argv[3]));
        else if(command == L"pause-render" && argc==4) pause_watch(std::stoul(argv[2]), std::stoul(argv[3]),true);
        else if(command==L"ping"&&argc==3){DWORD_PTR result=0;require(SendMessageTimeoutW(reinterpret_cast<HWND>(std::stoull(argv[2])),WM_NULL,0,0,SMTO_ABORTIFHUNG|SMTO_BLOCK,1000,&result)!=0,"Game window is not responding");std::cout<<"responsive";}
        else if(command == L"resume" && argc==4) resume_game(std::stoul(argv[2]), std::stoul(argv[3]));
        else if(command == L"detach" && argc==3) {
            auto directory=session(std::stoul(argv[2]));
            write(directory/L"disable.request", "1");
            bool disabled=false;
            for(int i=0;i<200;++i){if(read(directory/L"hook.status")=="disabled"){disabled=true;break;}Sleep(25);}
            require(disabled,"Render hook disable timed out");
        }
        else throw std::runtime_error("Invalid native command arguments");
        return 0;
    } catch(const winrt::hresult_error& error) { std::cerr << winrt::to_string(error.message()); }
    catch(const std::exception& error) { std::cerr << error.what(); }
    return 1;
}
