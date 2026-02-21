using System;
using System.Diagnostics;
using System.Collections.Generic;
using System.Linq;
using System.Security.Principal;
using System.Text;
using System.IO;
namespace NetworkOptimizer
{
    class Program
    {
        static void Main(string[] args)
        {
            // Forzar codificación ASCII para evitar caracteres raros
            Console.OutputEncoding = Encoding.ASCII;

            // 1. Verificacion de Administrador
            if (!IsAdministrator())
            {
                Console.ForegroundColor = ConsoleColor.Red;
                Console.WriteLine("ERROR: Este programa debe ejecutarse como Administrador.");
                Console.WriteLine("Por favor, click derecho -> Ejecutar como administrador.");
                Console.ResetColor();
                Console.WriteLine("\nPresiona cualquier tecla para salir...");
                Console.ReadKey();
                return;
            }

            Console.Title = "RS Optimizer";
            
            // Cabecera en ASCII simple para evitar errores visuales
            Console.ForegroundColor = ConsoleColor.Cyan;
            Console.WriteLine("+--------------------------------------------------+");
            Console.WriteLine("|                  RS Optimizer                    |");
            Console.WriteLine("|                by RickStyles                     |");
            Console.WriteLine("+--------------------------------------------------+");
            Console.ResetColor();

            Console.WriteLine("Optimizando parametros globales de Loopback...");
            try 
            {
                // Ejecutamos los comandos para prevenir errores en Steam, Google Agent y Localhost
                RunPowerShellCommand("netsh int ipv4 set global loopbacklargemtu=disable");
                RunPowerShellCommand("netsh int ipv6 set global loopbacklargemtu=disable");
    
                Console.ForegroundColor = ConsoleColor.Green;
                Console.WriteLine("[ OK ] Parametros de compatibilidad aplicados.");
                Console.ResetColor();
            }
            catch (Exception ex)
            {
                Console.ForegroundColor = ConsoleColor.Yellow;
                Console.WriteLine($"(!) No se pudieron aplicar ajustes de loopback: {ex.Message}");
                Console.ResetColor();
            }

            Console.WriteLine("\nDetectando perfiles TCP disponibles...");

            try
            {
                // 2. Obtener lista REAL de perfiles usando PowerShell (mas fiable que netsh)
                List<string> profiles = GetTcpProfiles();

                if (profiles.Count == 0)
                {
                    // Fallback de seguridad
                    profiles = new List<string> { "Internet", "Datacenter", "Compat", "InternetCustom", "DatacenterCustom" };
                    Console.ForegroundColor = ConsoleColor.Yellow;
                    Console.WriteLine("(!) No se pudo leer la lista dinamica. Usando lista predefinida.");
                    Console.ResetColor();
                }

                Console.WriteLine($"Se encontraron {profiles.Count} perfiles. Iniciando optimizacion...\n");

                // 3. Aplicar BBR a cada perfil
                int successCount = 0;
                foreach (string profile in profiles)
                {
                    // Intentamos aplicar BBR
                    if (ApplyBBR(profile))
                    {
                        successCount++;
                    }
                }

                Console.WriteLine("\n----------------------------------------------------");
                Console.ForegroundColor = ConsoleColor.Green;
                Console.WriteLine($"Resumen: {successCount} de {profiles.Count} perfiles actualizados a BBR.");
                Console.ResetColor();

                // 4. Verificacion Final Limpia
                Console.WriteLine("\n[Estado Final de la Configuracion]");
                Console.WriteLine("(Muestra solo el perfil 'Internet' como referencia)");
                
                // Mostramos el estado limpio sin caracteres raros
                string finalStatus = RunPowerShellCommand("Get-NetTCPSetting -SettingName Internet | Select-Object SettingName, CongestionProvider | Format-Table -AutoSize | Out-String");
                
                Console.ForegroundColor = ConsoleColor.White;
                Console.WriteLine(finalStatus.Trim());
                Console.ResetColor();

                Console.WriteLine("\n----------------------------------------------------");
                Console.WriteLine("Buscando y aplicando configuraciones de registro...");
                ApplyEmbeddedRegistryFiles();

            }
            catch (Exception ex)
            {
                Console.ForegroundColor = ConsoleColor.Red;
                Console.WriteLine($"\nError Critico: {ex.Message}");
            }
            finally
            {
                Console.ResetColor();
                Console.WriteLine("\nPresiona cualquier tecla para cerrar...");
                Console.ReadKey();
            }
        }

        // Extrae y ejecuta silenciosamente cualquier archivo .reg incrustado
        static void ApplyEmbeddedRegistryFiles()
        {
            try
            {
                var assembly = System.Reflection.Assembly.GetExecutingAssembly();
                var resourceNames = assembly.GetManifestResourceNames();
                bool foundReg = false;
                
                foreach (var resourceName in resourceNames)
                {
                    if (resourceName.EndsWith(".reg", StringComparison.OrdinalIgnoreCase))
                    {
                        foundReg = true;
                        Console.Write($" Aplicando archivo incrustado... ".PadRight(40));
                        
                        string tempFile = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString() + ".reg");
                        
                        using (Stream stream = assembly.GetManifestResourceStream(resourceName))
                        {
                            if (stream == null) continue;
                            using (FileStream fileStream = new FileStream(tempFile, FileMode.Create, FileAccess.Write))
                            {
                                stream.CopyTo(fileStream);
                            }
                        }

                        // Ejecutar regedit silenciosamente
                        ProcessStartInfo psi = new ProcessStartInfo();
                        psi.FileName = "regedit.exe";
                        psi.Arguments = $"/s \"{tempFile}\"";
                        psi.UseShellExecute = false;
                        psi.CreateNoWindow = true;

                        using (Process process = Process.Start(psi))
                        {
                            process.WaitForExit();
                        }

                        // Limpiar archivo temporal
                        if (File.Exists(tempFile))
                        {
                            try { File.Delete(tempFile); } catch { }
                        }

                        Console.ForegroundColor = ConsoleColor.Green;
                        Console.WriteLine("[ OK ]");
                        Console.ResetColor();
                    }
                }

                if (!foundReg)
                {
                    Console.ForegroundColor = ConsoleColor.DarkGray;
                    Console.WriteLine("No se encontraron archivos .reg incrustados.");
                    Console.ResetColor();
                }
            }
            catch (Exception ex)
            {
                Console.ForegroundColor = ConsoleColor.Yellow;
                Console.WriteLine($"\n(!) Error aplicando registro incrustado: {ex.Message}");
                Console.ResetColor();
            }
        }

        // Obtiene la lista de perfiles usando PowerShell
        static List<string> GetTcpProfiles()
        {
            List<string> list = new List<string>();
            try
            {
                // Comando para sacar solo los nombres unicos
                string output = RunPowerShellCommand("Get-NetTCPSetting | Select-Object -ExpandProperty SettingName -Unique");
                
                string[] lines = output.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries);
                foreach (string line in lines)
                {
                    string cleanName = line.Trim();
                    if (!string.IsNullOrEmpty(cleanName))
                    {
                        list.Add(cleanName);
                    }
                }
            }
            catch
            {
                // Si falla, devuelve lista vacia para usar el fallback
            }
            return list;
        }

        // Aplica BBR usando PowerShell y netsh como fallback
        static bool ApplyBBR(string profileName)
        {
            Console.Write($" Configurando '{profileName}'... ".PadRight(40));
            
            bool success = false;
            
            // Intento 1: netsh con bbr2 (Windows 11 / 10 moderno)
            try
            {
                string command = $"netsh int tcp set supplemental template=\"{profileName}\" congestionprovider=bbr2";
                RunPowerShellCommand(command);
                success = true;
            }
            catch { }

            // Intento 2: netsh con bbr (Legacy)
            if (!success)
            {
                try
                {
                    string command = $"netsh int tcp set supplemental template=\"{profileName}\" congestionprovider=bbr";
                    RunPowerShellCommand(command);
                    success = true;
                }
                catch { }
            }

            // Intento 3: PowerShell (Windows Server / Algunas versiones)
            if (!success)
            {
                try
                {
                    string command = $"Set-NetTCPSetting -SettingName \"{profileName}\" -CongestionProvider BBR -ErrorAction Stop";
                    RunPowerShellCommand(command);
                    success = true;
                }
                catch { }
            }

            if (!success)
            {
                try
                {
                    string command = $"Set-NetTCPSetting -SettingName \"{profileName}\" -CongestionProvider BBR2 -ErrorAction Stop";
                    RunPowerShellCommand(command);
                    success = true;
                }
                catch { }
            }

            if (success)
            {
                Console.ForegroundColor = ConsoleColor.Green;
                Console.WriteLine("[ OK ]");
                Console.ResetColor();
                return true;
            }
            else
            {
                // Si falla (comun en perfiles 'Automatic' o legados protegidos)
                Console.ForegroundColor = ConsoleColor.DarkGray;
                Console.WriteLine("[ OMITIDO / PROTEGIDO ]");
                Console.ResetColor();
                return false;
            }
        }

        // Metodo auxiliar para ejecutar comandos de PowerShell limpios
        static string RunPowerShellCommand(string command)
        {
            ProcessStartInfo psi = new ProcessStartInfo();
            psi.FileName = "powershell.exe";
            psi.Arguments = $"-NoProfile -ExecutionPolicy Bypass -Command \"{command}\"";
            psi.UseShellExecute = false;
            psi.RedirectStandardOutput = true;
            psi.CreateNoWindow = true; // Ocultar ventana negra extra

            using (Process process = Process.Start(psi))
            {
                string output = process.StandardOutput.ReadToEnd();
                process.WaitForExit();

                if (process.ExitCode != 0)
                {
                    throw new Exception("Error en comando PowerShell");
                }
                return output;
            }
        }

        static bool IsAdministrator()
        {
            using (WindowsIdentity identity = WindowsIdentity.GetCurrent())
            {
                WindowsPrincipal principal = new WindowsPrincipal(identity);
                return principal.IsInRole(WindowsBuiltInRole.Administrator);
            }
        }
    }
}