#!/usr/bin/env python3
"""Build an explicit local KIT candidate without modifying portable source pins."""
import json,os,pathlib,re,shutil,subprocess,sys,tempfile,tomllib
root=pathlib.Path(__file__).resolve().parents[1];kit=pathlib.Path(os.environ['STAR_KIT_CHECKOUT']).resolve();args=sys.argv[1:] or ['build']
version=tomllib.loads((kit/'Cargo.toml').read_text())['package']['version']
with tempfile.TemporaryDirectory(prefix='starmux-kit-candidate-') as directory:
    tree=pathlib.Path(directory);manifest=(root/'Cargo.toml').read_text()
    def candidate(match):
        value=re.sub(r'version = "[^"]+"','version = '+json.dumps(version),match[0])
        if os.environ.get('STAR_KIT_GPU')=='1':value=value.replace('"terminal-graphics"','"terminal-graphics", "gpu-renderer"')
        return value
    manifest=re.sub(r'(?m)^starkit = \{[^}]+\}',candidate,manifest);(tree/'Cargo.toml').write_text(manifest)
    if (root/'Cargo.lock').exists():shutil.copy2(root/'Cargo.lock',tree/'Cargo.lock')
    for name in ['src','scripts','vendor','terminal-client']:(tree/name).symlink_to(root/name,target_is_directory=True)
    patch='patch."https://github.com/bstar/starkit".starkit.path='+json.dumps(str(kit))
    env={**os.environ,'CARGO_TARGET_DIR':str(root/'target')}
    extra=['--features','lossless-canvas'] if args[0] in ['build','check','test','clippy','run'] and (kit/'src/terminal_graphics/pixel_stream.rs').exists() else []
    command=['cargo',args[0],'--manifest-path',str(tree/'Cargo.toml'),'--config',patch,*extra,*args[1:]]
    shell=root/'target/nix-shell';shell.mkdir(parents=True,exist_ok=True)
    for name in ['flake.nix','flake.lock']:shutil.copy2(root/name,shell/name)
    raise SystemExit(subprocess.run(['nix','--extra-experimental-features','nix-command flakes','develop','path:'+str(shell),'--profile',str(root/'target/dev-shell'),'-c',*command],cwd=root,env=env).returncode)
