// Owned D3D11 host for end-to-end injection, OCR, and watchdog validation.
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <d2d1.h>
#include <dwrite.h>
#include <wrl/client.h>
#include <string>
using Microsoft::WRL::ComPtr;
static LRESULT CALLBACK window_proc(HWND hwnd, UINT message, WPARAM w, LPARAM l) {
    if(message == WM_DESTROY){PostQuitMessage(0); return 0;} return DefWindowProcW(hwnd,message,w,l);
}
int WINAPI wWinMain(HINSTANCE instance,HINSTANCE,PWSTR,int) {
    WNDCLASSW klass{}; klass.hInstance=instance; klass.lpszClassName=L"TranslitTestHost"; klass.lpfnWndProc=window_proc;
    RegisterClassW(&klass);
    auto hwnd=CreateWindowW(klass.lpszClassName,L"Translit · DirectX 11 test game",WS_OVERLAPPEDWINDOW,150,120,1000,650,nullptr,nullptr,instance,nullptr);
    ShowWindow(hwnd,SW_SHOWNOACTIVATE);
    ShowWindow(hwnd,SW_SHOWNOACTIVATE); // A hidden launcher overrides only the first call.
    DXGI_SWAP_CHAIN_DESC desc{}; desc.BufferDesc.Width=1000; desc.BufferDesc.Height=650; desc.BufferDesc.Format=DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count=1; desc.BufferUsage=DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.BufferCount=1; desc.OutputWindow=hwnd; desc.Windowed=TRUE;
    ComPtr<IDXGISwapChain> chain; ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    if(FAILED(D3D11CreateDeviceAndSwapChain(nullptr,D3D_DRIVER_TYPE_HARDWARE,nullptr,D3D11_CREATE_DEVICE_BGRA_SUPPORT,nullptr,0,D3D11_SDK_VERSION,&desc,&chain,&device,nullptr,&context))) return 1;
    ComPtr<ID2D1Factory> factory; D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED,factory.GetAddressOf());
    ComPtr<IDXGISurface> surface; chain->GetBuffer(0,IID_PPV_ARGS(&surface));
    ComPtr<ID2D1RenderTarget> target; auto properties=D2D1::RenderTargetProperties(D2D1_RENDER_TARGET_TYPE_DEFAULT,D2D1::PixelFormat(DXGI_FORMAT_UNKNOWN,D2D1_ALPHA_MODE_IGNORE));
    factory->CreateDxgiSurfaceRenderTarget(surface.Get(),&properties,&target);
    ComPtr<IDWriteFactory> fonts; DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED,__uuidof(IDWriteFactory),reinterpret_cast<IUnknown**>(fonts.GetAddressOf()));
    ComPtr<IDWriteTextFormat> format; fonts->CreateTextFormat(L"Segoe UI",nullptr,DWRITE_FONT_WEIGHT_NORMAL,DWRITE_FONT_STYLE_NORMAL,DWRITE_FONT_STRETCH_NORMAL,28,L"en-US",&format);
    ComPtr<ID2D1SolidColorBrush> brush; target->CreateSolidColorBrush(D2D1::ColorF(D2D1::ColorF::White),&brush);
    MSG message{}; unsigned frame=0;
    while(message.message!=WM_QUIT) {
        while(PeekMessageW(&message,nullptr,0,0,PM_REMOVE)){TranslateMessage(&message);DispatchMessageW(&message);}
        target->BeginDraw(); target->Clear(D2D1::ColorF(0.06f,0.12f,0.18f));
        auto title=L"TRANSLIT / DIRECTX 11 TEST";
        target->DrawText(title,static_cast<UINT32>(wcslen(title)),format.Get(),D2D1::RectF(50,50,950,130),brush.Get());
        auto text=L"Estelle: We should investigate the mysterious ruins.\nJoshua: Remember to protect your companions.\nEvery journey begins with a single step.";
        target->DrawText(text,static_cast<UINT32>(wcslen(text)),format.Get(),D2D1::RectF(50,390,950,620),brush.Get());
        auto counter=std::to_wstring(frame++);
        target->DrawText(counter.c_str(),static_cast<UINT32>(counter.size()),format.Get(),D2D1::RectF(50,160,900,210),brush.Get());
        target->EndDraw(); chain->Present(1,0);
    }
    return 0;
}
