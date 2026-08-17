import type { JSX } from 'solid-js'

type IconProps = JSX.SvgSVGAttributes<SVGSVGElement>

function IconFrame(props: IconProps & { children: JSX.Element }) {
  const { children, ...rest } = props
  return (
    <svg
      viewBox="0 0 20 20"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {children}
    </svg>
  )
}

export function AgentsIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <circle cx="7" cy="7" r="2.5" />
      <path d="M2.8 15c.7-2.5 2.1-3.7 4.2-3.7s3.5 1.2 4.2 3.7" />
      <path d="M12.2 5.1a2.5 2.5 0 0 1 0 4.8M13.2 11.5c1.9.2 3.1 1.4 3.7 3.5" />
    </IconFrame>
  )
}

export function CloseIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <path d="m5 5 10 10M15 5 5 15" />
    </IconFrame>
  )
}

export function EditIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <path d="M4 14.8 5 11l7.8-7.8 4 4L9 15l-3.8 1Z" />
      <path d="m11.5 4.5 4 4" />
    </IconFrame>
  )
}

export function GripIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <circle cx="7" cy="5" r=".8" fill="currentColor" stroke="none" />
      <circle cx="13" cy="5" r=".8" fill="currentColor" stroke="none" />
      <circle cx="7" cy="10" r=".8" fill="currentColor" stroke="none" />
      <circle cx="13" cy="10" r=".8" fill="currentColor" stroke="none" />
      <circle cx="7" cy="15" r=".8" fill="currentColor" stroke="none" />
      <circle cx="13" cy="15" r=".8" fill="currentColor" stroke="none" />
    </IconFrame>
  )
}

export function HideIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <path d="M4 10h12" />
    </IconFrame>
  )
}

export function ChevronLeftIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <path d="m12.5 4.5-5 5.5 5 5.5" />
    </IconFrame>
  )
}

export function ChevronRightIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <path d="m7.5 4.5 5 5.5-5 5.5" />
    </IconFrame>
  )
}

export function CheckIcon(props: IconProps) {
  return (
    <IconFrame {...props}>
      <path d="m4 10 4 4 8-9" />
    </IconFrame>
  )
}
