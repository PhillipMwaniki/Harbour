import { useEffect, useRef, useState } from "react";

import type { Folder, Host, HostInput } from "@/ipc/types";
import { useThemeCatalogue } from "@/stores/settings";

interface Props {
  /** The host being edited, or `null` when adding a new one. */
  host: Host | null;
  folders: Folder[];
  /** Where a new host lands, when a folder is selected in the tree. */
  defaultFolderId: string | null;
  /** The theme this host overrides the app theme with, if any. */
  themeId?: string | null;
  onSave: (input: HostInput, themeId: string | null) => void;
  onCancel: () => void;
  /** Only offered for an existing host that has something saved. */
  onForgetSecrets?: () => void;
}

const DEFAULT_PORT = 22;

/**
 * Add or edit a saved host.
 *
 * There is no password field, deliberately. A password is asked for by the
 * connection, at the moment it is needed, and saved from there if the user
 * chooses; typing one into a form here would mean holding it in the webview
 * with nothing to do with it yet.
 */
export function HostDialog({
  host,
  folders,
  defaultFolderId,
  themeId,
  onSave,
  onCancel,
  onForgetSecrets,
}: Props) {
  const [name, setName] = useState(host?.name ?? "");
  const [hostname, setHostname] = useState(host?.hostname ?? "");
  const [port, setPort] = useState(String(host?.port ?? DEFAULT_PORT));
  const [username, setUsername] = useState(host?.username ?? "");
  const [description, setDescription] = useState(host?.description ?? "");
  const [folderId, setFolderId] = useState(host?.folderId ?? defaultFolderId ?? "");
  const [useAgent, setUseAgent] = useState(host?.auth.useAgent ?? true);
  const [keyPath, setKeyPath] = useState(host?.auth.keyPath ?? "");
  const [usePassword, setUsePassword] = useState(host?.auth.usePassword ?? true);
  const [themeOverride, setThemeOverride] = useState(themeId ?? "");
  const themes = useThemeCatalogue();
  const hostnameRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    hostnameRef.current?.focus();
  }, []);

  const parsedPort = Number.parseInt(port, 10);
  const portValid = Number.isInteger(parsedPort) && parsedPort > 0 && parsedPort <= 65535;
  // A host with nothing enabled would fail on connect with a message about
  // methods, which is a poor way to find out the form was incomplete.
  const hasMethod = useAgent || keyPath.trim() !== "" || usePassword;
  const canSave = hostname.trim() !== "" && username.trim() !== "" && portValid && hasMethod;

  const submit = () => {
    if (!canSave) return;
    onSave({
      folderId: folderId || null,
      name: name.trim() || hostname.trim(),
      hostname: hostname.trim(),
      port: parsedPort,
      username: username.trim(),
      description: description.trim() || null,
      auth: {
        useAgent,
        keyPath: keyPath.trim() || null,
        usePassword,
      },
    }, themeOverride || null);
  };

  return (
    <div
      className="absolute inset-0 z-30 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <form
        role="dialog"
        aria-label={host ? "Edit host" : "Add host"}
        className="max-h-[90%] w-[26rem] overflow-y-auto rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 shadow-xl"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
        }}
      >
        <h2 className="mb-3 text-sm font-medium">{host ? "Edit host" : "Add host"}</h2>

        <Field label="Host" htmlFor="host-hostname">
          <input
            id="host-hostname"
            ref={hostnameRef}
            value={hostname}
            onChange={(event) => setHostname(event.target.value)}
            placeholder="server.example.com"
            className={inputClass}
          />
        </Field>

        <div className="mb-3 flex gap-3">
          <div className="flex-1">
            <Field label="Username" htmlFor="host-username">
              <input
                id="host-username"
                value={username}
                onChange={(event) => setUsername(event.target.value)}
                className={inputClass}
              />
            </Field>
          </div>
          <div className="w-20">
            <Field label="Port" htmlFor="host-port">
              <input
                id="host-port"
                value={port}
                inputMode="numeric"
                aria-invalid={!portValid}
                onChange={(event) => setPort(event.target.value)}
                className={inputClass}
              />
            </Field>
          </div>
        </div>

        <Field label="Name" htmlFor="host-name" hint="defaults to the hostname">
          <input
            id="host-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            className={inputClass}
          />
        </Field>

        <Field label="Folder" htmlFor="host-folder">
          <select
            id="host-folder"
            value={folderId}
            onChange={(event) => setFolderId(event.target.value)}
            className={inputClass}
          >
            <option value="">(top level)</option>
            {folders.map((folder) => (
              <option key={folder.id} value={folder.id}>
                {folder.name}
              </option>
            ))}
          </select>
        </Field>

        <Field
          label="Theme"
          htmlFor="host-theme"
          hint="optional"
        >
          <select
            id="host-theme"
            value={themeOverride}
            onChange={(event) => setThemeOverride(event.target.value)}
            className={inputClass}
          >
            <option value="">(the app theme)</option>
            {themes.map((theme) => (
              <option key={theme.id} value={theme.id}>
                {theme.label}
              </option>
            ))}
          </select>
        </Field>

        <Field label="Description" htmlFor="host-description" hint="optional">
          <input
            id="host-description"
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            className={inputClass}
          />
        </Field>

        <fieldset className="mb-3 rounded border border-[var(--hb-border)] p-2">
          <legend className="px-1 text-xs text-[var(--hb-fg-muted)]">Authentication</legend>

          <label className="mb-2 flex items-center gap-2">
            <input
              type="checkbox"
              checked={useAgent}
              onChange={(event) => setUseAgent(event.target.checked)}
            />
            Use the SSH agent
          </label>

          <Field label="Private key" htmlFor="host-key" hint="optional">
            <input
              id="host-key"
              value={keyPath}
              onChange={(event) => setKeyPath(event.target.value)}
              placeholder="~/.ssh/id_ed25519"
              className={inputClass}
            />
          </Field>

          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={usePassword}
              onChange={(event) => setUsePassword(event.target.checked)}
            />
            Ask for a password
          </label>

          {!hasMethod && (
            <p role="alert" className="mt-2 text-[var(--hb-danger)]">
              Enable at least one way to authenticate.
            </p>
          )}
        </fieldset>

        {host?.hasSavedPassword && onForgetSecrets && (
          <div className="mb-3 flex items-center justify-between text-[var(--hb-fg-muted)]">
            <span>A password is saved in the system keychain.</span>
            <button
              type="button"
              onClick={onForgetSecrets}
              className="rounded px-2 py-1 hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
            >
              Forget it
            </button>
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!canSave}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            Save
          </button>
        </div>
      </form>
    </div>
  );
}

const inputClass =
  "w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs";

function Field({
  label,
  htmlFor,
  hint,
  children,
}: {
  label: string;
  htmlFor: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-3">
      <label className="mb-1 block text-xs" htmlFor={htmlFor}>
        {label}
        {hint && <span className="ml-1 text-[var(--hb-fg-muted)]">({hint})</span>}
      </label>
      {children}
    </div>
  );
}
