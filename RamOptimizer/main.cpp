#include <windows.h>
#include <psapi.h>
#include <shellapi.h>
#include <tlhelp32.h>

#pragma comment(lib, "psapi.lib")
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "shell32.lib")

#define WM_TRAYICON (WM_USER + 1)
#define ID_TRAY_APP_ICON 1001
#define ID_TRAY_EXIT 1002
#define TIMER_OPTIMIZE 2001

HINSTANCE g_hInstance;
HWND g_hwnd;
NOTIFYICONDATAW g_nid;

// Función que vacía el Working Set de los procesos, enviando la memoria a paginación/compresión de Windows
void OptimizeRAM() {
    DWORD aProcesses[1024], cbNeeded, cProcesses;
    
    // EnumProcesses es muy ligero y no satura la CPU
    if (!EnumProcesses(aProcesses, sizeof(aProcesses), &cbNeeded)) {
        return;
    }

    cProcesses = cbNeeded / sizeof(DWORD);
    DWORD currentId = GetCurrentProcessId();

    for (unsigned int i = 0; i < cProcesses; i++) {
        if (aProcesses[i] != 0 && aProcesses[i] != currentId) {
            // Abrimos el proceso pidiendo solo privilegios mínimos para modificar su cuota de memoria
            HANDLE hProcess = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION, FALSE, aProcesses[i]);
            if (hProcess) {
                // Forzamos a Windows a vaciar el "Working Set" asignado al proceso. 
                // Esto es lo que hacen los programas limpiadores de RAM (ej. Razer Cortex, MemReduct).
                EmptyWorkingSet(hProcess);
                CloseHandle(hProcess);
            }
        }
    }
}

LRESULT CALLBACK WindowProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
        case WM_CREATE:
            // Configurar el timer para que corra cada 3 minutos (180,000 milisegundos)
            SetTimer(hwnd, TIMER_OPTIMIZE, 180000, NULL);
            // Ejecutar la primera optimización apenas inicie
            OptimizeRAM();
            return 0;

        case WM_TIMER:
            if (wParam == TIMER_OPTIMIZE) {
                OptimizeRAM(); // Se ejecuta de forma automática muy rápido y en silencio
            }
            return 0;

        case WM_TRAYICON:
            if (lParam == WM_RBUTTONUP || lParam == WM_LBUTTONUP) {
                POINT pt;
                GetCursorPos(&pt);
                HMENU hMenu = CreatePopupMenu();
                InsertMenuW(hMenu, 0, MF_BYPOSITION | MF_STRING, ID_TRAY_EXIT, L"Cerrar");
                
                SetForegroundWindow(hwnd);
                TrackPopupMenu(hMenu, TPM_BOTTOMALIGN | TPM_LEFTALIGN, pt.x, pt.y, 0, hwnd, NULL);
                DestroyMenu(hMenu);
            }
            return 0;

        case WM_COMMAND:
            if (LOWORD(wParam) == ID_TRAY_EXIT) {
                DestroyWindow(hwnd);
            }
            return 0;

        case WM_DESTROY:
            KillTimer(hwnd, TIMER_OPTIMIZE);
            Shell_NotifyIconW(NIM_DELETE, &g_nid);
            PostQuitMessage(0);
            return 0;
    }
    return DefWindowProcW(hwnd, uMsg, wParam, lParam);
}

// Punto de entrada para aplicaciones gráficas Win32 (evita que se abra una ventana de consola)
int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPSTR lpCmdLine, int nCmdShow) {
    // 1. Terminar cualquier instancia anterior de RS RAM Optimizer antes de iniciar
    DWORD currentPid = GetCurrentProcessId();
    HANDLE hSnap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (hSnap != INVALID_HANDLE_VALUE) {
        PROCESSENTRY32 pe;
        pe.dwSize = sizeof(PROCESSENTRY32);
        if (Process32First(hSnap, &pe)) {
            do {
                // Comparamos el nombre del exe, ignorando mayusculas/minusculas
                if (_stricmp(pe.szExeFile, "RS RAM Optimizer.exe") == 0 && pe.th32ProcessID != currentPid) {
                    HANDLE hProc = OpenProcess(PROCESS_TERMINATE, FALSE, pe.th32ProcessID);
                    if (hProc) {
                        TerminateProcess(hProc, 0);
                        CloseHandle(hProc);
                    }
                }
            } while (Process32Next(hSnap, &pe));
        }
        CloseHandle(hSnap);
    }
    // Breve pausa para que Windows libere los recursos del proceso anterior
    Sleep(200);

    // 2. Adquirir Mutex (seguridad extra por si acaso)
    HANDLE hMutex = CreateMutex(NULL, TRUE, "RS_RamOptimizer_Mutex_Global_Unique");
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        CloseHandle(hMutex);
        return 0;
    }

    g_hInstance = hInstance;

    const wchar_t CLASS_NAME[] = L"RS_TrayOptimizerClass";

    WNDCLASSW wc = { };
    wc.lpfnWndProc   = WindowProc;
    wc.hInstance     = hInstance;
    // Añadimos el icono a la clase de ventana para que no quede vacio en algunas partes de Windows
    wc.hIcon         = LoadIcon(hInstance, MAKEINTRESOURCE(101)); // 101 suele ser el ID que los compiladores asignan por defecto si no definimos un ID numerico
    wc.lpszClassName = CLASS_NAME;

    RegisterClassW(&wc);

    // Creamos una "Message-only window", esto significa que la ventana existe solo para recibir eventos
    // pero nunca se dibuja en pantalla ni consume recursos gráficos.
    g_hwnd = CreateWindowExW(
        0,
        CLASS_NAME,
        L"RS Tray Optimizer",
        0,
        0, 0, 0, 0,
        HWND_MESSAGE, NULL, hInstance, NULL
    );

    if (g_hwnd == NULL) {
        CloseHandle(hMutex);
        return 0;
    }

    g_nid.cbSize = sizeof(NOTIFYICONDATAW);
    g_nid.hWnd = g_hwnd;
    g_nid.uID = ID_TRAY_APP_ICON;
    g_nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    g_nid.uCallbackMessage = WM_TRAYICON;
    
    // Obtenemos el icono personalizado incrustado en el archivo .exe a traves del archivo resource.rc
    g_nid.hIcon = LoadIcon(hInstance, MAKEINTRESOURCE(101)); 
    
    // Si el icono fallara por alguna razon (e.g. compilado sin el .rc), usamos el generico de app como fallback
    if (g_nid.hIcon == NULL) {
        g_nid.hIcon = LoadIcon(NULL, IDI_APPLICATION);
    }
    
    lstrcpyW(g_nid.szTip, L"RS RAM Optimizer (Automático)");

    Shell_NotifyIconW(NIM_ADD, &g_nid);

    // Bucle de mensajes nativo (Consume < 1 MB de RAM y 0% de CPU en pausa)
    MSG msg = { };
    while (GetMessageW(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    // Limpieza final antes de salir (aunque Windows deberia matar el Handle de forma automatica)
    CloseHandle(hMutex);
    return 0;
}
