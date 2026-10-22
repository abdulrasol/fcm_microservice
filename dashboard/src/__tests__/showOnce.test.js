import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import ShowOnceModal from '../components/ShowOnceModal.vue'

const SECRET = 'fcm_watt_abcdefghijklmnopqrstuvwxyz012345'

describe('the modal of a secret that is shown once', () => {
  it('show_once_modal_clears_secret_on_close', async () => {
    const wrapper = mount(ShowOnceModal, {
      props: { open: true, secret: SECRET },
    })
    expect(wrapper.find('#show-once-secret').element.value).toBe(SECRET)

    // The close button waits for the confirmation box.
    const close = wrapper.findAll('button').find((button) => button.text().includes('إغلاق'))
    expect(close.attributes('disabled')).toBeDefined()
    await close.trigger('click')
    expect(wrapper.emitted('close')).toBeFalsy()

    await wrapper.find('input[type="checkbox"]').setValue(true)
    await close.trigger('click')
    expect(wrapper.emitted('close')).toBeTruthy()
    expect(wrapper.vm.value).toBe('')
  })

  it('show_once_modal_clears_secret_on_unmount', () => {
    const wrapper = mount(ShowOnceModal, {
      props: { open: true, secret: SECRET },
    })
    expect(wrapper.vm.value).toBe(SECRET)
    wrapper.unmount()
    expect(wrapper.vm.value).toBe('')
  })

  it('show_once_secret_not_in_route_or_storage', () => {
    const wrapper = mount(ShowOnceModal, {
      props: { open: true, secret: SECRET },
    })

    // The secret is nowhere but the field of the modal.
    expect(window.location.href).not.toContain(SECRET)
    expect(window.location.search).not.toContain(SECRET)
    expect(window.localStorage.length).toBe(0)
    expect(window.sessionStorage.length).toBe(0)
    expect(document.cookie).not.toContain(SECRET)
    expect(wrapper.html()).toContain(SECRET)
    wrapper.unmount()
  })
})
