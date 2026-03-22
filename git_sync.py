import os
import subprocess
import datetime
import sys
import time

def run_command(command, description, ignore_error=False):
    print(f"\n🚀 {description}...")
    try:
        # Run command and capture output
        result = subprocess.run(command, shell=True, check=True, text=True, capture_output=True)
        print(f"✅ Éxito: {result.stdout.strip()}")
        return True
    except subprocess.CalledProcessError as e:
        if not ignore_error:
            print(f"❌ Error al ejecutar '{command}':")
            print(e.stderr)
        else:
            print(f"⚠️ {description} falló (posiblemente requiere pull).")
        return False

def main():
    print("="*60)
    print("🤖 Forsaken GitHub Sync (Prioridad: Local)")
    print("="*60)
    
    # 1. Check status
    if not run_command("git status", "Verificando estado"):
        return

    # 2. Add changes
    if not run_command("git add .", "Añadiendo archivos"):
        return
        
    # 3. Commit
    timestamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    commit_msg = f"Auto-sync: {timestamp}"
    # Permitimos fallo en commit si no hay cambios, pero igual intentamos push
    run_command(f'git commit -m "{commit_msg}"', f"Haciendo commit ('{commit_msg}')", ignore_error=True)
    
    # 4. INTENTO 1: Push directo (Prioridad al local)
    print("\n⬆️ Intentando subir cambios (Push directo)...")
    if run_command("git push origin main", "Push inicial", ignore_error=True):
        print("\n✨ ¡Sincronización completada con éxito! (Push directo)")
        time.sleep(2)
        return

    # 5. Si falla el push, probablemente necesitamos pull
    print("\n⚠️ El push directo falló. Probablemente hay cambios remotos.")
    print("🔄 Sincronizando con remoto (Pull)...")
    
    if run_command("git pull origin main", "Descargando cambios (Pull)"):
        # 6. INTENTO 2: Push después de pull
        if run_command("git push origin main", "Push final"):
            print("\n✨ ¡Sincronización completada con éxito! (Pull + Push)")
        else:
            print("\n❌ Error fatal: No se pudo subir ni después de hacer pull.")
    else:
        print("\n❌ Error en el Pull. Revisa conflictos manualmente.")

    input("\nPresiona Enter para salir...")

if __name__ == "__main__":
    main()
