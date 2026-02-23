#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif

#include <windows.h>
#include <psapi.h>
#include <shellapi.h>
#include <tlhelp32.h>
#include <pdh.h>
#include <pdhmsg.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#pragma comment(lib, "psapi.lib")
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "shell32.lib")
#pragma comment(lib, "pdh.lib")

// ==============================================================
//  MODO DEBUG: Cambiar a 0 para produccion (sin consola)
// ==============================================================
#define DEBUG_MODE 0

// ==============================================================
//  CONSTANTES
// ==============================================================
#define WM_TRAYICON         (WM_USER + 1)
#define ID_TRAY_APP_ICON    1001
#define ID_TRAY_EXIT        1002
#define TIMER_OPTIMIZE      2001
#define MAX_PROCS           1024
#define OPTIMIZE_INTERVAL   180000  // 3 minutos en ms
#define CPU_SAMPLE_DELAY    500     // Delay entre muestras CPU en ms
#define SCORE_THRESHOLD     100.0   // Puntos minimos para considerar "juego"

// Pesos del sistema de scoring
#define W_CPU       3.0
#define W_RAM       1.5
#define W_GPU       4.0
#define BONUS_FS    20.0    // Bonus por pantalla completa

// Macro de debug
#if DEBUG_MODE
  #define DBG(fmt, ...) printf(fmt, ##__VA_ARGS__)
#else
  #define DBG(fmt, ...) ((void)0)
#endif

// ==============================================================
//  ESTRUCTURAS
// ==============================================================
typedef struct {
    DWORD pid;
    char  name[MAX_PATH];
    ULONGLONG cpuKernel1, cpuUser1;
    ULONGLONG cpuKernel2, cpuUser2;
    double cpuPercent;
    double ramMB;
    double gpuPercent;
    BOOL   isFullscreen;
    double score;
} ProcessInfo;

typedef struct {
    DWORD pid;
    double gpuPercent;
} GpuPidEntry;

// ==============================================================
//  GLOBALES
// ==============================================================
HINSTANCE g_hInstance;
HWND g_hwnd;
NOTIFYICONDATAW g_nid;
DWORD g_protectedPid = 0;

// Arrays estaticos para evitar overflow de stack
static ProcessInfo g_procs[MAX_PROCS];
static GpuPidEntry g_gpuTable[MAX_PROCS];

// Puntero a PdhAddEnglishCounterA (carga dinamica para compatibilidad con MinGW)
typedef PDH_STATUS (WINAPI *PFN_PdhAddEnglishCounterA)(
    PDH_HQUERY, LPCSTR, DWORD_PTR, PDH_HCOUNTER*);
static PFN_PdhAddEnglishCounterA g_pfnAddEngCounter = NULL;
static BOOL g_pdhChecked = FALSE;

// ==============================================================
//  LISTA NEGRA: Procesos que NUNCA se consideran "juego"
// ==============================================================
static const char* BLACKLIST[] = {
    "chrome.exe", "firefox.exe", "msedge.exe", "opera.exe", "brave.exe",
    "explorer.exe", "discord.exe", "spotify.exe", "steam.exe", "steamwebhelper.exe",
    "svchost.exe", "csrss.exe", "dwm.exe", "system", "smss.exe",
    "wininit.exe", "winlogon.exe", "lsass.exe", "services.exe",
    "taskhostw.exe", "runtimebroker.exe", "searchhost.exe",
    "shellexperiencehost.exe", "startmenuexperiencehost.exe",
    "textinputhost.exe", "widgetservice.exe", "widgets.exe",
    "sihost.exe", "fontdrvhost.exe", "ctfmon.exe",
    "securityhealthservice.exe", "securityhealthsystray.exe",
    "msedgewebview2.exe", "gameinputsvc.exe",
    "rs ram optimizer.exe", "conhost.exe", "dllhost.exe",
    "wmiprvse.exe", "searchindexer.exe", "msiexec.exe", "audiodg.exe",
    "code.exe", "devenv.exe", "taskmgr.exe",
    "wallpaper32.exe", "wallpaper64.exe", "wallpaperservice32.exe",
    NULL
};

// Palabras clave: cualquier proceso cuyo nombre CONTENGA estas palabras se ignora
static const char* BLACKLIST_PARTIAL[] = {
    "wallpaper",
    NULL
};

// ==============================================================
//  FUNCIONES AUXILIARES
// ==============================================================
static BOOL IsBlacklisted(const char* procName) {
    // Coincidencia exacta
    for (int i = 0; BLACKLIST[i] != NULL; i++) {
        if (_stricmp(procName, BLACKLIST[i]) == 0)
            return TRUE;
    }
    // Coincidencia parcial (contiene la palabra clave)
    // Convertir nombre a minusculas para busqueda case-insensitive
    char lower[MAX_PATH];
    strncpy(lower, procName, MAX_PATH - 1);
    lower[MAX_PATH - 1] = '\0';
    for (char* p = lower; *p; p++) *p = (char)tolower((unsigned char)*p);

    for (int i = 0; BLACKLIST_PARTIAL[i] != NULL; i++) {
        if (strstr(lower, BLACKLIST_PARTIAL[i]) != NULL)
            return TRUE;
    }
    return FALSE;
}

static inline ULONGLONG FT2ULL(FILETIME ft) {
    ULARGE_INTEGER li;
    li.LowPart = ft.dwLowDateTime;
    li.HighPart = ft.dwHighDateTime;
    return li.QuadPart;
}

static int GetNumCPUs() {
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    return (int)si.dwNumberOfProcessors;
}

static double GetTotalRAM_MB() {
    MEMORYSTATUSEX ms;
    ms.dwLength = sizeof(ms);
    GlobalMemoryStatusEx(&ms);
    return (double)ms.ullTotalPhys / (1024.0 * 1024.0);
}

// Extrae PID del nombre de instancia GPU Engine: "pid_1234_luid_..."
static DWORD ParsePidFromGpuInstance(const char* name) {
    const char* p = strstr(name, "pid_");
    if (p) {
        p += 4;
        return (DWORD)atoi(p);
    }
    return 0;
}

// Detecta si la ventana en primer plano ocupa todo el monitor (fullscreen)
static DWORD GetFullscreenPID() {
    HWND fg = GetForegroundWindow();
    if (!fg) return 0;

    RECT wndRect;
    GetWindowRect(fg, &wndRect);

    HMONITOR hMon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
    MONITORINFO mi;
    mi.cbSize = sizeof(MONITORINFO);
    GetMonitorInfo(hMon, &mi);
    RECT scrRect = mi.rcMonitor;

    if (wndRect.left <= scrRect.left && wndRect.top <= scrRect.top &&
        wndRect.right >= scrRect.right && wndRect.bottom >= scrRect.bottom) {
        DWORD pid = 0;
        GetWindowThreadProcessId(fg, &pid);
        return pid;
    }
    return 0;
}

// Inicializa el puntero a PdhAddEnglishCounterA de forma dinamica
static void InitPdhEnglish() {
    if (g_pdhChecked) return;
    g_pdhChecked = TRUE;
    HMODULE hPdh = GetModuleHandleA("pdh.dll");
    if (!hPdh) hPdh = LoadLibraryA("pdh.dll");
    if (hPdh) {
        g_pfnAddEngCounter = (PFN_PdhAddEnglishCounterA)
            GetProcAddress(hPdh, "PdhAddEnglishCounterA");
    }
    DBG("  [PDH] PdhAddEnglishCounterA: %s\n",
        g_pfnAddEngCounter ? "disponible" : "NO disponible (fallback a PdhAddCounterA)");
}

// Wrapper que usa PdhAddEnglishCounterA si existe, sino PdhAddCounterA
static PDH_STATUS SafeAddCounter(PDH_HQUERY q, LPCSTR path, DWORD_PTR ud, PDH_HCOUNTER* c) {
    InitPdhEnglish();
    if (g_pfnAddEngCounter)
        return g_pfnAddEngCounter(q, path, ud, c);
    return PdhAddCounterA(q, path, ud, c);
}

// ==============================================================
//  DETECCION DE GPU VIA PDH PERFORMANCE COUNTERS
// ==============================================================
static int QueryGPU(GpuPidEntry* table, int maxEntries) {
    int count = 0;
    PDH_HQUERY query = NULL;
    PDH_HCOUNTER counter = NULL;

    if (PdhOpenQueryA(NULL, 0, &query) != ERROR_SUCCESS) {
        DBG("  [GPU] PdhOpenQuery fallo - GPU scoring desactivado\n");
        return 0;
    }

    PDH_STATUS st = SafeAddCounter(query,
        "\\GPU Engine(*)\\Utilization Percentage", 0, &counter);
    if (st != ERROR_SUCCESS) {
        DBG("  [GPU] No se pudo agregar counter GPU (0x%lx)\n", st);
        DBG("  [GPU] (Normal si Windows < 10 1709 o sin GPU compatible)\n");
        PdhCloseQuery(query);
        return 0;
    }

    // PDH necesita 2 recolecciones para calcular rates
    PdhCollectQueryData(query);
    Sleep(250);
    if (PdhCollectQueryData(query) != ERROR_SUCCESS) {
        PdhCloseQuery(query);
        return 0;
    }

    // Obtener tamano del buffer
    DWORD bufSize = 0, itemCount = 0;
    st = PdhGetFormattedCounterArrayA(counter, PDH_FMT_DOUBLE,
        &bufSize, &itemCount, NULL);
    if (st != PDH_MORE_DATA && st != ERROR_SUCCESS) {
        PdhCloseQuery(query);
        return 0;
    }

    PDH_FMT_COUNTERVALUE_ITEM_A* items =
        (PDH_FMT_COUNTERVALUE_ITEM_A*)malloc(bufSize);
    if (!items) { PdhCloseQuery(query); return 0; }

    st = PdhGetFormattedCounterArrayA(counter, PDH_FMT_DOUBLE,
        &bufSize, &itemCount, items);

    if (st == ERROR_SUCCESS) {
        for (DWORD i = 0; i < itemCount && count < maxEntries; i++) {
            if (items[i].FmtValue.CStatus != PDH_CSTATUS_VALID_DATA)
                continue;
            DWORD pid = ParsePidFromGpuInstance(items[i].szName);
            if (pid == 0) continue;
            double val = items[i].FmtValue.doubleValue;
            if (val < 0.01) continue;

            // Buscar PID existente y sumar (un proceso puede usar multiples engines)
            BOOL found = FALSE;
            for (int j = 0; j < count; j++) {
                if (table[j].pid == pid) {
                    table[j].gpuPercent += val;
                    found = TRUE;
                    break;
                }
            }
            if (!found) {
                table[count].pid = pid;
                table[count].gpuPercent = val;
                count++;
            }
        }
        // Cap a 100%
        for (int j = 0; j < count; j++) {
            if (table[j].gpuPercent > 100.0)
                table[j].gpuPercent = 100.0;
        }
    }

    free(items);
    PdhCloseQuery(query);
    return count;
}

static double FindGpuForPid(GpuPidEntry* table, int count, DWORD pid) {
    for (int i = 0; i < count; i++) {
        if (table[i].pid == pid)
            return table[i].gpuPercent;
    }
    return 0.0;
}

// ==============================================================
//  OPTIMIZACION INTELIGENTE
// ==============================================================
void SmartOptimize() {
    DWORD pids[MAX_PROCS];
    DWORD cbNeeded;
    if (!EnumProcesses(pids, sizeof(pids), &cbNeeded))
        return;

    DWORD numProcs = cbNeeded / sizeof(DWORD);
    DWORD currentPid = GetCurrentProcessId();
    int numCPUs = GetNumCPUs();
    double totalRAM = GetTotalRAM_MB();

    DBG("\n========================================\n");
    DBG("  CICLO DE OPTIMIZACION INTELIGENTE\n");
    DBG("========================================\n");
    DBG("  CPUs: %d | RAM Total: %.0f MB | Procesos: %lu\n\n", numCPUs, totalRAM, numProcs);

    // --- PASO 1: GPU Query ---
    DBG("  [1/5] Consultando GPU via PDH...\n");
    int gpuCount = QueryGPU(g_gpuTable, MAX_PROCS);
    DBG("  [GPU] %d procesos con actividad GPU\n\n", gpuCount);

    // --- PASO 2: CPU Muestra 1 + RAM ---
    DBG("  [2/5] Muestra CPU #1...\n");
    int validCount = 0;
    ULONGLONG wallClock1 = GetTickCount64();

    for (DWORD i = 0; i < numProcs && validCount < MAX_PROCS; i++) {
        if (pids[i] == 0 || pids[i] == currentPid) continue;

        HANDLE hProc = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SET_QUOTA,
            FALSE, pids[i]);
        if (!hProc) continue;

        char name[MAX_PATH] = {0};
        if (GetModuleBaseNameA(hProc, NULL, name, MAX_PATH) == 0) {
            CloseHandle(hProc);
            continue;
        }

        FILETIME ct, et, kt, ut;
        if (GetProcessTimes(hProc, &ct, &et, &kt, &ut)) {
            ProcessInfo* p = &g_procs[validCount];
            p->pid = pids[i];
            strncpy(p->name, name, MAX_PATH - 1);
            p->cpuKernel1 = FT2ULL(kt);
            p->cpuUser1 = FT2ULL(ut);

            PROCESS_MEMORY_COUNTERS pmc;
            if (GetProcessMemoryInfo(hProc, &pmc, sizeof(pmc)))
                p->ramMB = (double)pmc.WorkingSetSize / (1024.0 * 1024.0);
            else
                p->ramMB = 0;

            p->gpuPercent = 0;
            p->isFullscreen = FALSE;
            p->score = 0;
            validCount++;
        }
        CloseHandle(hProc);
    }

    // --- PASO 3: Esperar delta ---
    DBG("  [3/5] Esperando %d ms para delta CPU...\n", CPU_SAMPLE_DELAY);
    Sleep(CPU_SAMPLE_DELAY);

    // --- PASO 4: CPU Muestra 2 + Calcular scores ---
    DBG("  [4/5] Muestra CPU #2 + scoring...\n");
    ULONGLONG wallClock2 = GetTickCount64();
    ULONGLONG wallDelta = (wallClock2 - wallClock1) * 10000ULL; // ms -> 100ns
    if (wallDelta == 0) wallDelta = 1;

    DWORD fsPid = GetFullscreenPID();

    for (int i = 0; i < validCount; i++) {
        ProcessInfo* p = &g_procs[i];

        HANDLE hProc = OpenProcess(PROCESS_QUERY_INFORMATION, FALSE, p->pid);
        if (hProc) {
            FILETIME ct, et, kt, ut;
            if (GetProcessTimes(hProc, &ct, &et, &kt, &ut)) {
                p->cpuKernel2 = FT2ULL(kt);
                p->cpuUser2 = FT2ULL(ut);
                ULONGLONG cpuDelta = (p->cpuKernel2 - p->cpuKernel1) +
                                     (p->cpuUser2 - p->cpuUser1);
                p->cpuPercent = ((double)cpuDelta / (double)wallDelta) / numCPUs * 100.0;
                if (p->cpuPercent > 100.0) p->cpuPercent = 100.0;
                if (p->cpuPercent < 0.0)   p->cpuPercent = 0.0;
            }
            CloseHandle(hProc);
        }

        p->gpuPercent = FindGpuForPid(g_gpuTable, gpuCount, p->pid);
        p->isFullscreen = (p->pid == fsPid && fsPid != 0);

        // Calcular score
        if (IsBlacklisted(p->name)) {
            p->score = -1.0;
        } else {
            double ramPct = (p->ramMB / totalRAM) * 100.0;
            p->score = (p->cpuPercent * W_CPU) +
                       (ramPct * W_RAM) +
                       (p->gpuPercent * W_GPU);
            if (p->isFullscreen)
                p->score += BONUS_FS;
        }
    }

    // --- PASO 5: Encontrar ganador y aplicar ---
    DBG("  [5/5] Evaluando scores...\n\n");

    int bestIdx = -1;
    double bestScore = 0;
    for (int i = 0; i < validCount; i++) {
        if (g_procs[i].score > bestScore) {
            bestScore = g_procs[i].score;
            bestIdx = i;
        }
    }

#if DEBUG_MODE
    // Mostrar top 15 procesos ordenados por score
    {
        static int indices[MAX_PROCS];
        for (int i = 0; i < validCount; i++) indices[i] = i;
        for (int i = 0; i < validCount - 1; i++) {
            for (int j = i + 1; j < validCount; j++) {
                if (g_procs[indices[j]].score > g_procs[indices[i]].score) {
                    int tmp = indices[i]; indices[i] = indices[j]; indices[j] = tmp;
                }
            }
        }

        DBG("  %-28s %7s %8s %7s %3s %9s\n",
            "PROCESO", "CPU%", "RAM(MB)", "GPU%", "FS", "SCORE");
        DBG("  %-28s %7s %8s %7s %3s %9s\n",
            "----------------------------", "-------", "--------", "-------", "---", "---------");

        int show = validCount < 15 ? validCount : 15;
        for (int i = 0; i < show; i++) {
            int idx = indices[i];
            if (g_procs[idx].score < 0) continue;
            DBG("  %-28s %6.1f%% %7.1f %6.1f%% %3s %9.1f%s\n",
                g_procs[idx].name,
                g_procs[idx].cpuPercent,
                g_procs[idx].ramMB,
                g_procs[idx].gpuPercent,
                g_procs[idx].isFullscreen ? "SI" : "NO",
                g_procs[idx].score,
                (idx == bestIdx && bestScore >= SCORE_THRESHOLD) ? " <-- JUEGO" : "");
        }
        DBG("\n");
    }
#endif

    // Determinar juego activo
    DWORD gamePid = 0;
    if (bestIdx >= 0 && bestScore >= SCORE_THRESHOLD) {
        gamePid = g_procs[bestIdx].pid;
        g_protectedPid = gamePid;
        DBG("  >>> JUEGO DETECTADO: %s (PID %lu, Score %.1f)\n",
            g_procs[bestIdx].name, gamePid, bestScore);

        // Subir prioridad del juego
        HANDLE hGame = OpenProcess(PROCESS_SET_INFORMATION, FALSE, gamePid);
        if (hGame) {
            SetPriorityClass(hGame, HIGH_PRIORITY_CLASS);
            DBG("  >>> Prioridad elevada a HIGH_PRIORITY_CLASS\n");
            CloseHandle(hGame);
        }
    } else {
        g_protectedPid = 0;
        DBG("  >>> No se detecto juego (Score max: %.1f, Threshold: %.1f)\n",
            bestScore, SCORE_THRESHOLD);
    }

    // Aplicar EmptyWorkingSet a todos excepto el juego
    int optimized = 0, skipped = 0;
    for (int i = 0; i < validCount; i++) {
        if (g_procs[i].pid == gamePid && gamePid != 0) {
            skipped++;
            continue;
        }
        HANDLE hProc = OpenProcess(PROCESS_SET_QUOTA, FALSE, g_procs[i].pid);
        if (hProc) {
            EmptyWorkingSet(hProc);
            CloseHandle(hProc);
            optimized++;
        }
    }

    DBG("\n  Resultado: %d optimizados, %d protegidos\n", optimized, skipped);
    DBG("========================================\n\n");
}

// ==============================================================
//  WINDOW PROCEDURE
// ==============================================================
LRESULT CALLBACK WindowProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
        case WM_CREATE:
            SetTimer(hwnd, TIMER_OPTIMIZE, OPTIMIZE_INTERVAL, NULL);
            SmartOptimize();
            return 0;

        case WM_TIMER:
            if (wParam == TIMER_OPTIMIZE)
                SmartOptimize();
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
            if (LOWORD(wParam) == ID_TRAY_EXIT)
                DestroyWindow(hwnd);
            return 0;

        case WM_DESTROY:
            KillTimer(hwnd, TIMER_OPTIMIZE);
            Shell_NotifyIconW(NIM_DELETE, &g_nid);
            PostQuitMessage(0);
            return 0;
    }
    return DefWindowProcW(hwnd, uMsg, wParam, lParam);
}

// ==============================================================
//  PUNTO DE ENTRADA
// ==============================================================
int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPSTR lpCmdLine, int nCmdShow) {

#if DEBUG_MODE
    AllocConsole();
    freopen("CONOUT$", "w", stdout);
    SetConsoleTitleA("RS RAM Optimizer - DEBUG MODE");
    printf("=== RS RAM Optimizer v2.0 - MODO DEBUG ===\n");
    printf("Esta ventana muestra el funcionamiento interno.\n");
    printf("Cambiar DEBUG_MODE a 0 en el codigo para produccion.\n\n");
#endif

    // 1. Terminar instancias anteriores
    DWORD currentPid = GetCurrentProcessId();
    HANDLE hSnap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (hSnap != INVALID_HANDLE_VALUE) {
        PROCESSENTRY32 pe;
        pe.dwSize = sizeof(PROCESSENTRY32);
        if (Process32First(hSnap, &pe)) {
            do {
                if (_stricmp(pe.szExeFile, "RS RAM Optimizer.exe") == 0 &&
                    pe.th32ProcessID != currentPid) {
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
    Sleep(200);

    // 2. Mutex para instancia unica
    HANDLE hMutex = CreateMutex(NULL, TRUE, "RS_RamOptimizer_Mutex_Global_Unique");
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        CloseHandle(hMutex);
        return 0;
    }

    g_hInstance = hInstance;
    const wchar_t CLASS_NAME[] = L"RS_TrayOptimizerClass";

    WNDCLASSW wc = { };
    wc.lpfnWndProc   = WindowProc;
    wc.hInstance      = hInstance;
    wc.hIcon          = LoadIcon(hInstance, MAKEINTRESOURCE(101));
    wc.lpszClassName  = CLASS_NAME;
    RegisterClassW(&wc);

    g_hwnd = CreateWindowExW(
        0, CLASS_NAME, L"RS Tray Optimizer", 0,
        0, 0, 0, 0,
        HWND_MESSAGE, NULL, hInstance, NULL
    );

    if (g_hwnd == NULL) {
        CloseHandle(hMutex);
        return 0;
    }

    // Icono en bandeja del sistema
    g_nid.cbSize = sizeof(NOTIFYICONDATAW);
    g_nid.hWnd = g_hwnd;
    g_nid.uID = ID_TRAY_APP_ICON;
    g_nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    g_nid.uCallbackMessage = WM_TRAYICON;

    g_nid.hIcon = LoadIcon(hInstance, MAKEINTRESOURCE(101));
    if (g_nid.hIcon == NULL)
        g_nid.hIcon = LoadIcon(NULL, IDI_APPLICATION);

    lstrcpyW(g_nid.szTip, L"RS RAM Optimizer v2.0 (Smart)");
    Shell_NotifyIconW(NIM_ADD, &g_nid);

    DBG("Icono de bandeja creado. Intervalo: %d seg\n\n", OPTIMIZE_INTERVAL / 1000);

    MSG msg = { };
    while (GetMessageW(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

#if DEBUG_MODE
    FreeConsole();
#endif
    CloseHandle(hMutex);
    return 0;
}
