import { render } from 'solid-js/web'

import { App } from './app/App'
import './styles.css'

const root = document.getElementById('root')
if (!root) {
  throw new Error('Liveboard root element is missing')
}

render(() => <App />, root)
