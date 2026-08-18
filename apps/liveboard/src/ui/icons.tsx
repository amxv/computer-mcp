import agentsSvg from 'lucide-static/icons/users.svg?raw'
import arrowDownSvg from 'lucide-static/icons/arrow-down.svg?raw'
import checkSvg from 'lucide-static/icons/check.svg?raw'
import chevronDownSvg from 'lucide-static/icons/chevron-down.svg?raw'
import chevronUpSvg from 'lucide-static/icons/chevron-up.svg?raw'
import closeSvg from 'lucide-static/icons/x.svg?raw'
import copySvg from 'lucide-static/icons/copy.svg?raw'
import editSvg from 'lucide-static/icons/pencil.svg?raw'
import folderSvg from 'lucide-static/icons/folder.svg?raw'
import gripSvg from 'lucide-static/icons/grip-vertical.svg?raw'
import hideSvg from 'lucide-static/icons/minus.svg?raw'
import cogSvg from 'lucide-static/icons/settings.svg?raw'
import terminalSvg from 'lucide-static/icons/terminal.svg?raw'
import userSvg from 'lucide-static/icons/user.svg?raw'

function normalizedLucideSvg(svg: string) {
  return svg.replace('width="24"', 'width="16"').replace('height="24"', 'height="16"')
}

function RawLucideIcon(props: { svg: string }) {
  return (
    <span
      class="lucide-icon"
      aria-hidden="true"
      innerHTML={normalizedLucideSvg(props.svg)}
    />
  )
}

export function AgentsIcon() {
  return <RawLucideIcon svg={agentsSvg} />
}

export function ArrowDownIcon() {
  return <RawLucideIcon svg={arrowDownSvg} />
}

export function UserIcon() {
  return <RawLucideIcon svg={userSvg} />
}

export function CloseIcon() {
  return <RawLucideIcon svg={closeSvg} />
}

export function EditIcon() {
  return <RawLucideIcon svg={editSvg} />
}

export function GripIcon() {
  return <RawLucideIcon svg={gripSvg} />
}

export function HideIcon() {
  return <RawLucideIcon svg={hideSvg} />
}

export function ChevronDownIcon() {
  return <RawLucideIcon svg={chevronDownSvg} />
}

export function ChevronUpIcon() {
  return <RawLucideIcon svg={chevronUpSvg} />
}

export function FolderIcon() {
  return <RawLucideIcon svg={folderSvg} />
}

export function TerminalIcon() {
  return <RawLucideIcon svg={terminalSvg} />
}

export function CheckIcon() {
  return <RawLucideIcon svg={checkSvg} />
}

export function CopyIcon() {
  return <RawLucideIcon svg={copySvg} />
}

export function CogIcon() {
  return <RawLucideIcon svg={cogSvg} />
}
