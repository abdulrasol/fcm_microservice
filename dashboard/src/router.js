// The routes of the dashboard.

import { createRouter, createWebHistory } from 'vue-router'
import { canAccess, isOwner, load, session } from './session'
import { decideRoute } from './lib/guard'
import Login from './views/Login.vue'
import ChangePassword from './views/ChangePassword.vue'
import NotFound from './views/NotFound.vue'

const routes = [
  { path: '/login', name: 'login', component: Login },
  { path: '/password', name: 'password', component: ChangePassword, meta: { requiresAuth: true } },
  { path: '/:pathMatch(.*)*', name: 'not-found', component: NotFound, meta: { requiresAuth: true } },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

router.beforeEach(async (to) => {
  if (!session.loaded) {
    await load()
  }
  const decision = decideRoute({
    to,
    me: session.me,
    owner: isOwner.value,
    canAccess,
  })
  if (!decision) {
    return true
  }
  if (decision.message) {
    router.state.__message = decision.message
  }
  return { path: decision.redirect }
})

export default router
